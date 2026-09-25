//! Single-thread engine preview: the owning thread runs QuickJS, and every call crosses
//! the Rust/JS boundary natively, without generated source or JSON text.

use indexmap::IndexMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use rquickjs::{
    Array, CatchResultExt, CaughtError, Context, Ctx, Function, Module, Object, Persistent,
    Runtime, Value as JsValue,
};

use crate::compiler::CompilerService;
use crate::config::VmOptions;
use crate::contract::{JsArgs, JsDecode, JsEncode, array_length};
use crate::error::VmError;
use crate::registry::{HostModuleStyle, InMemoryHostContractRegistry};
use crate::types::ScriptId;

use super::bridge_capability::{WorkerBridgeCapability, ensure_host_contracts_supported};
use super::errors::{caught_js_error, js_error};
use super::execution::{ExecutionControl, ExecutionGuard};
use super::module_loader::{
    MemoryModuleLoader, MemoryModuleResolver, RuntimeModuleGraph, WorkerModuleStore,
};
use super::promise_rejections::UnhandledRejections;
use super::render::bootstrap_module_context_source;

const NATIVE_FUNCTIONS_GLOBAL: &str = "__rustts_native";
const HANDLERS_GLOBAL: &str = "__vm_handlers";

/// Installs `__host` and the namespaced host globals on top of `__rustts_native`.
const HOST_GLOBALS_SOURCE: &str = include_str!("../../assets/engine_host_globals.js");

/// Single-thread RustTS engine.
///
/// The engine owns its QuickJS runtime and cannot leave the thread that created it.
/// Script exports are called directly, host functions are native QuickJS functions,
/// and values convert without JSON text.
///
/// Every load, call and emit runs under `VmOptions::execution_timeout`: JavaScript
/// still running when it expires is interrupted and the operation fails with
/// `VmError::Execution`. Like the worker pool, the budget is cooperative and cannot
/// preempt a Rust host function.
///
/// Promise jobs an operation queues run before it returns, within the same budget:
/// an `async` export resolves to its value, and a Promise rejection that no handler
/// caught by then fails the operation.
///
/// Current limits: inline scripts only, no disk cache, and no Promise-returning host
/// functions, so a Promise that only a host could settle fails the call. Use
/// [`RustTs`](crate::RustTs) for multi-file projects, worker threads and async host
/// functions.
pub struct Engine {
    /// In load order, which is the order events are delivered in.
    scripts: IndexMap<ScriptId, EngineScript>,
    registry: Arc<InMemoryHostContractRegistry>,
    compiler: CompilerService,
    module_store: WorkerModuleStore,
    next_graph_id: u64,
    execution: Arc<ExecutionControl>,
    execution_timeout: Duration,
    // Declared last: contexts and persistent values must drop before their runtime.
    rejections: UnhandledRejections,
    runtime: Runtime,
}

struct EngineScript {
    context: Context,
    exports: Persistent<Object<'static>>,
    module_ids: Vec<String>,
}

impl Engine {
    /// Creates an engine on the current thread using the memory, stack and validation
    /// settings from `options`.
    pub fn new(options: &VmOptions) -> Result<Self, VmError> {
        let module_store = WorkerModuleStore::default();
        let execution = Arc::new(ExecutionControl::default());
        let runtime = new_runtime(options, &module_store, &execution)?;
        Ok(Self {
            scripts: IndexMap::new(),
            registry: Arc::new(InMemoryHostContractRegistry::with_validation_options(
                options.contract_validation,
                options.unknown_field_validation,
            )),
            compiler: CompilerService::default(),
            module_store,
            next_graph_id: 0,
            execution,
            execution_timeout: options.execution_timeout,
            rejections: UnhandledRejections::install(&runtime),
            runtime,
        })
    }

    /// Host contract registry. Register contracts before loading the scripts that use them.
    pub fn registry(&self) -> &InMemoryHostContractRegistry {
        &self.registry
    }

    /// Loads or replaces one TypeScript script. A failed load keeps the previous version.
    pub fn load_script(&mut self, id: impl Into<ScriptId>, source: &str) -> Result<(), VmError> {
        let id = id.into();
        ensure_host_contracts_supported(&self.registry, WorkerBridgeCapability::Sync)?;
        let transpiled = self.transpile(&id, source)?;
        let graph = self.install_graph(&id, transpiled)?;
        match self.mount(&graph.entry_module_id) {
            Ok((context, exports)) => self.replace_script(
                id,
                EngineScript {
                    context,
                    exports,
                    module_ids: graph.module_ids,
                },
            ),
            Err(error) => {
                self.module_store.remove_modules(&graph.module_ids)?;
                Err(error)
            }
        }
    }

    /// Unloads one script and releases its modules.
    pub fn unload_script(&mut self, id: &str) -> Result<(), VmError> {
        let script = self
            .scripts
            .shift_remove(id)
            .ok_or_else(|| script_not_found(id))?;
        self.module_store.remove_modules(&script.module_ids)
    }

    /// Calls one exported function. Arguments encode through [`JsArgs`] (a tuple, a
    /// `Vec` or a slice) and the result decodes through [`JsDecode`], natively on both
    /// sides; `call::<serde_json::Value>(id, name, vec![json])` keeps a JSON-shaped API.
    /// An `async` export resolves before its value is decoded.
    pub fn call<R: JsDecode>(
        &self,
        script_id: &str,
        function: &str,
        args: impl JsArgs,
    ) -> Result<R, VmError> {
        let script = self.script(script_id)?;
        let _budget = self.budget();
        let result = script.context.with(|ctx| {
            let export = script.export(&ctx, script_id, function)?;
            let args = args.encode_args(&ctx).map_err(js_error)?;
            let returned = export
                .call_arg::<JsValue<'_>>(args)
                .catch(&ctx)
                .map_err(caught_js_error)?;
            let value = self.resolve_returned(&ctx, returned, script_id, function)?;
            R::decode_js(&ctx, value)
                .catch(&ctx)
                .map_err(caught_js_error)
        });
        self.settle(result)
    }

    /// Delivers one event to every handler registered for it, script by script in load
    /// order; returns the number of scripts that had at least one handler. A throwing
    /// handler does not stop the others: every handler runs, then the first error is
    /// returned. The payload is encoded once per script with handlers and shared by all
    /// of that script's handlers.
    pub fn emit<P: JsEncode + ?Sized>(&self, event: &str, payload: &P) -> Result<usize, VmError> {
        let _budget = self.budget();
        let mut delivered = 0;
        let mut first_error = None;
        for script in self.scripts.values() {
            match script.context.with(|ctx| deliver(&ctx, event, payload)) {
                Ok(true) => delivered += 1,
                Ok(false) => {}
                Err(error) => {
                    delivered += 1;
                    first_error.get_or_insert(error);
                }
            }
        }
        self.settle(first_error.map_or(Ok(delivered), Err))
    }

    /// Awaits a Promise returned by an export by running the job queue. Its rejection
    /// is the call's error, so it is not also reported as unhandled.
    fn resolve_returned<'js>(
        &self,
        ctx: &Ctx<'js>,
        returned: JsValue<'js>,
        script_id: &str,
        function: &str,
    ) -> Result<JsValue<'js>, VmError> {
        let Some(promise) = returned.as_promise() else {
            return Ok(returned);
        };
        let settled = promise.finish::<JsValue<'_>>().catch(ctx);
        self.rejections.forget(ctx, &returned);
        settled.map_err(|error| match error {
            CaughtError::Error(rquickjs::Error::WouldBlock) => VmError::Execution {
                details: format!(
                    "function `{function}` of script `{script_id}` returned a Promise that never settles: only script Promise jobs run on an Engine"
                ),
            },
            error => caught_js_error(error),
        })
    }

    /// Runs every job left in the queue, for all scripts, then reports in order: the
    /// operation's own error, a job that threw, a Promise rejection nobody handled.
    /// Runs even when the operation failed, so nothing leaks into the next one.
    fn settle<T>(&self, result: Result<T, VmError>) -> Result<T, VmError> {
        let mut job_error = None;
        loop {
            match self.runtime.execute_pending_job() {
                Ok(true) => {}
                Ok(false) => break,
                Err(exception) => {
                    let error = exception.0.with(|ctx| {
                        caught_js_error(CaughtError::from_error(&ctx, rquickjs::Error::Exception))
                    });
                    job_error.get_or_insert(error);
                }
            }
        }
        let unhandled = self.rejections.take();
        let value = result?;
        match job_error.or(unhandled) {
            Some(error) => Err(error),
            None => Ok(value),
        }
    }

    fn transpile(&mut self, id: &str, source: &str) -> Result<String, VmError> {
        let source_path = Path::new(id).with_extension("ts");
        self.compiler
            .compile_script(String::new(), source, &source_path, PathBuf::new())
            .map(|compiled| compiled.transpiled_js)
    }

    fn install_graph(
        &mut self,
        id: &str,
        transpiled: String,
    ) -> Result<RuntimeModuleGraph, VmError> {
        self.module_store
            .insert_host_modules(self.registry.import_modules(HostModuleStyle::Native)?)?;
        let graph_id = self.next_graph_id;
        self.next_graph_id += 1;
        self.module_store.insert_inline(id, transpiled, graph_id)
    }

    fn mount(
        &self,
        entry_module_id: &str,
    ) -> Result<(Context, Persistent<Object<'static>>), VmError> {
        let _budget = self.budget();
        let context = Context::full(&self.runtime).map_err(js_error)?;
        let exports = context.with(|ctx| {
            evaluate_script(&ctx, bootstrap_module_context_source())?;
            self.install_native_functions(&ctx)?;
            evaluate_script(&ctx, HOST_GLOBALS_SOURCE)?;
            import_exports(&ctx, entry_module_id)
        });
        Ok((context, self.settle(exports)?))
    }

    fn install_native_functions(&self, ctx: &Ctx<'_>) -> Result<(), VmError> {
        let functions = Object::new(ctx.clone()).map_err(js_error)?;
        self.registry
            .install_native_functions(&functions)
            .map_err(js_error)?;
        ctx.globals()
            .set(NATIVE_FUNCTIONS_GLOBAL, functions)
            .map_err(js_error)
    }

    fn replace_script(&mut self, id: ScriptId, script: EngineScript) -> Result<(), VmError> {
        let Some(previous) = self.scripts.insert(id.clone(), script) else {
            return Ok(());
        };
        self.module_store.remove_modules(&previous.module_ids)
    }

    fn script(&self, id: &str) -> Result<&EngineScript, VmError> {
        self.scripts.get(id).ok_or_else(|| script_not_found(id))
    }

    /// Starts the execution budget for one operation; it ends when the guard drops.
    fn budget(&self) -> ExecutionGuard<'_> {
        self.execution.enter(self.execution_timeout)
    }
}

impl EngineScript {
    fn export<'js>(
        &self,
        ctx: &Ctx<'js>,
        script_id: &str,
        name: &str,
    ) -> Result<Function<'js>, VmError> {
        let exports = self.exports.clone().restore(ctx).map_err(js_error)?;
        exports
            .get::<_, Option<Function<'js>>>(name)
            .map_err(js_error)?
            .ok_or_else(|| VmError::FunctionNotFound {
                script_id: script_id.to_owned(),
                function_name: name.to_owned(),
            })
    }
}

fn new_runtime(
    options: &VmOptions,
    module_store: &WorkerModuleStore,
    execution: &Arc<ExecutionControl>,
) -> Result<Runtime, VmError> {
    let runtime = Runtime::new().map_err(js_error)?;
    runtime.set_memory_limit(options.memory_limit_bytes);
    runtime.set_max_stack_size(options.max_stack_size_bytes);
    runtime.set_loader(
        MemoryModuleResolver::new(module_store.clone()),
        MemoryModuleLoader::new(module_store.clone()),
    );
    let interrupt = execution.clone();
    runtime.set_interrupt_handler(Some(Box::new(move || interrupt.interrupted())));
    Ok(runtime)
}

fn evaluate_script(ctx: &Ctx<'_>, source: &str) -> Result<(), VmError> {
    ctx.eval::<(), _>(source)
        .catch(ctx)
        .map_err(caught_js_error)
}

fn import_exports(
    ctx: &Ctx<'_>,
    entry_module_id: &str,
) -> Result<Persistent<Object<'static>>, VmError> {
    let exports = Module::import(ctx, entry_module_id)
        .and_then(|promise| promise.finish::<Object<'_>>())
        .catch(ctx)
        .map_err(caught_js_error)?;
    Ok(Persistent::save(ctx, exports))
}

/// Runs every handler the script registered for `event`, even after one throws; returns
/// whether there were any, or the first handler error. An entry that is not a function
/// means the script corrupted its handler list, and stops delivery to it at once.
fn deliver<P: JsEncode + ?Sized>(ctx: &Ctx<'_>, event: &str, payload: &P) -> Result<bool, VmError> {
    let Some(handlers) = event_handlers(ctx, event)? else {
        return Ok(false);
    };
    let payload = payload.encode_js(ctx).map_err(js_error)?;
    // The handler list is script-visible; read its length without trusting it fits i32.
    let count = array_length(&handlers, "event handlers").map_err(js_error)?;
    let mut first_error = None;
    for index in 0..count {
        let handler = handlers.get::<Function<'_>>(index).map_err(js_error)?;
        if let Err(error) = handler
            .call::<_, ()>((payload.clone(),))
            .catch(ctx)
            .map_err(caught_js_error)
        {
            first_error.get_or_insert(error);
        }
    }
    first_error.map_or(Ok(true), Err)
}

fn event_handlers<'js>(ctx: &Ctx<'js>, event: &str) -> Result<Option<Array<'js>>, VmError> {
    ctx.globals()
        .get::<_, Object<'js>>(HANDLERS_GLOBAL)
        .and_then(|handlers| handlers.get::<_, Option<Array<'js>>>(event))
        .map_err(js_error)
}

fn script_not_found(id: &str) -> VmError {
    VmError::ScriptNotFound {
        script_id: id.to_owned(),
    }
}
