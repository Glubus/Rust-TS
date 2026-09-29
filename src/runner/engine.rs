//! Single-thread engine: the owning thread runs QuickJS, and every call crosses the
//! Rust/JS boundary natively, without generated source or JSON text.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use indexmap::IndexMap;
use rquickjs::{
    CatchResultExt, CaughtError, Context, Ctx, Function, Module, Object, Persistent, Runtime,
    Value as JsValue,
};

use crate::compiler::WatchedFiles;
use crate::config::VmOptions;
use crate::contract::{JsArgs, JsDecode, JsEncode};
use crate::error::VmError;
use crate::registry::InMemoryHostContractRegistry;
use crate::types::{MemoryStats, ReloadReport, ScriptId};

use super::console::{ConsoleLevel, ConsoleSink};
use super::errors::{caught_js_error, in_typescript, js_error};
use super::events::{ListenedEvents, deliver};
use super::execution::{ExecutionControl, ExecutionGuard};
use super::interrupt::InterruptHandle;
use super::memory::memory_stats;
use super::module_loader::{
    MemoryModuleLoader, MemoryModuleResolver, RuntimeModuleGraph, WorkerModuleStore,
};
use super::promise_rejections::UnhandledRejections;
use super::timers::{NextTimer, TimerClock, install_timer_hooks, run_due_timers};
use super::transpile::Transpiler;

const NATIVE_FUNCTIONS_GLOBAL: &str = "__rustts_native";
/// Frozen `{ save, dispose }` hooks the prelude installs behind `ctx.hot`.
const HOT_GLOBAL: &str = "__rustts_hot";
const HOT_SAVE: &str = "save";
const HOT_DISPOSE: &str = "dispose";
/// Carries the previous version's saved state into the prelude, which moves it to
/// `ctx.hot.data`.
const HOT_DATA_GLOBAL: &str = "__rustts_hot_data";

/// Installs `ctx.on` / `ctx.off` and their handler lists, `ctx.hot`, `console`, the
/// timer functions, `__host` and the namespaced host globals on top of the native hooks.
const CONTEXT_PRELUDE: &str = include_str!("../../assets/context_prelude.js");

/// Single-thread RustTS engine.
///
/// The engine owns its QuickJS runtime and cannot leave the thread that created it.
/// Script exports are called directly, host functions are native QuickJS functions,
/// and values convert without JSON text.
///
/// Every load, call, emit, request and timer advance runs under
/// `VmOptions::execution_timeout`: JavaScript still running when it expires is
/// interrupted and the operation fails with `VmError::Execution`.
/// [`Engine::interrupt_handle`] stops it earlier from another thread. Both are
/// cooperative and cannot preempt a Rust host function.
///
/// Promise jobs an operation queues run before it returns, within the same budget:
/// an `async` export resolves to its value, and a Promise rejection that no handler
/// caught by then fails the operation. Host functions are synchronous, so a Promise
/// that only a host could settle fails the call.
///
/// Scripts get `setTimeout`, `setInterval`, `clearTimeout` and `clearInterval` on an
/// engine clock that only [`Engine::advance_timers`] moves, and a `console` whose
/// output goes to stderr unless [`Engine::set_console`] routes it elsewhere.
///
/// Replacing a loaded script, through [`Engine::load_script`], [`Engine::load_project`]
/// or [`Engine::reload_changed`], hands over the state the script opts to keep with
/// `ctx.hot`: the loaded version's `ctx.hot.save` callback runs first, and its result
/// is the new version's `ctx.hot.data` before any of its code runs. Once the new
/// version is loaded, the old version's `ctx.hot.dispose` callbacks run, in
/// registration order. A throwing `save` fails the load and keeps the old version; a
/// throwing `dispose` fails the load call although the new version stays loaded.
/// [`Engine::unload_script`] runs the `dispose` callbacks too; dropping the engine
/// does not.
pub struct Engine {
    /// In load order, which is the order events are delivered in.
    scripts: IndexMap<ScriptId, EngineScript>,
    registry: Arc<InMemoryHostContractRegistry>,
    transpiler: Transpiler,
    module_store: WorkerModuleStore,
    next_graph_id: u64,
    execution: Arc<ExecutionControl>,
    execution_timeout: Duration,
    console: ConsoleSink,
    timer_clock: TimerClock,
    // Declared last: contexts and persistent values must drop before their runtime.
    rejections: UnhandledRejections,
    runtime: Runtime,
}

struct EngineScript {
    context: Context,
    exports: Persistent<Object<'static>>,
    signals: ScriptSignals,
    module_ids: Vec<String>,
    origin: ScriptOrigin,
}

/// What a script's context reports to the engine as it registers handlers and timers,
/// so `emit` and `advance_timers` skip scripts with nothing to run instead of entering
/// their context, and find the handlers to run without a lookup by name. Owned by the
/// script: a reload starts anew.
#[derive(Default)]
struct ScriptSignals {
    events: ListenedEvents,
    next_timer: NextTimer,
}

/// The value a replaced version's `ctx.hot.save` returned, on its way to the next
/// version's `ctx.hot.data`. Contexts share the runtime, so it is the object itself.
type HotData = Persistent<JsValue<'static>>;

/// Outcome of retiring a replaced or unloaded version: its `ctx.hot.dispose` error.
type Disposed = Result<(), VmError>;

/// Where a script's code came from, kept for hot reload and the transpile memo.
struct ScriptOrigin {
    /// Content keys of its transpiled modules.
    module_keys: Vec<String>,
    /// Entry file and watched files of a project; `None` for inline scripts.
    project: Option<ProjectFiles>,
}

struct ProjectFiles {
    entry_path: PathBuf,
    watched: WatchedFiles,
}

impl Engine {
    /// Creates an engine on the current thread using the memory, stack, cache and
    /// validation settings from `options`.
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
            transpiler: Transpiler::new(options.cache_dir.as_deref())?,
            module_store,
            next_graph_id: 0,
            execution,
            execution_timeout: options.execution_timeout,
            console: ConsoleSink::default(),
            timer_clock: TimerClock::default(),
            rejections: UnhandledRejections::install(&runtime),
            runtime,
        })
    }

    /// Host contract registry. Register contracts before loading the scripts that use them.
    pub fn registry(&self) -> &InMemoryHostContractRegistry {
        &self.registry
    }

    /// Routes the `console` output of every script, loaded or to come, to `sink`, which
    /// receives the level, the id of the calling script and the message: the
    /// arguments of the call joined by spaces, strings as they are, other values as
    /// JSON when they have one. Stack locations point at the TypeScript source.
    /// Without a sink, output goes to stderr as `[level] script: message`.
    pub fn set_console(&mut self, sink: impl Fn(ConsoleLevel, &str, &str) + 'static) {
        self.console.replace(sink);
    }

    /// Moves the engine clock forward by `elapsed` and fires the timers now due, script
    /// by script in load order, each script's in due order; returns the number of
    /// scripts that had timers to fire. Call it from the host loop, typically once per
    /// frame with the frame time; nothing else moves the clock, so timers are
    /// deterministic.
    ///
    /// A timer fires at most once per call: one a callback schedules waits for the
    /// next call even with a zero delay, and an interval late by several periods fires
    /// once, its next due time staying its previous one plus its delay. A throwing
    /// callback does not stop the others; the first error is returned. Timers belong
    /// to the script version that set them: a reload or unload drops them.
    pub fn advance_timers(&self, elapsed: Duration) -> Result<usize, VmError> {
        let now = self.timer_clock.advance(elapsed);
        let due = self
            .scripts
            .values()
            .filter(|script| script.signals.next_timer.is_due(now));
        let _budget = self.budget();
        let mut fired = 0;
        let mut first_error = None;
        for script in due {
            fired += 1;
            if let Err(error) = script.context.with(|ctx| run_due_timers(&ctx, now)) {
                first_error.get_or_insert(error);
            }
        }
        if fired == 0 {
            // No JavaScript ran, so there is no Promise job to settle.
            return Ok(0);
        }
        self.attribute_interrupt(self.settle(first_error.map_or(Ok(fired), Err)))
    }

    /// Loads or replaces one TypeScript script. It may import host modules, not other
    /// files; use [`Engine::load_project`] for those. A failed load keeps the previous
    /// version; see [`Engine`] for the `ctx.hot` state a replaced version hands over.
    pub fn load_script(&mut self, id: impl Into<ScriptId>, source: &str) -> Result<(), VmError> {
        let id = id.into();
        let script = self.transpiler.inline(&id, source)?;
        self.install_host_modules()?;
        let graph_id = self.next_graph_id();
        let graph =
            self.module_store
                .insert_inline(&id, script.js, script.module_origin, graph_id)?;
        let origin = ScriptOrigin {
            module_keys: vec![script.module_key],
            project: None,
        };
        self.mount_graph(id, graph, origin)?
    }

    /// Loads or replaces one multi-file TypeScript project from its entry file.
    ///
    /// The static ESM graph is resolved from disk: relative imports, `tsconfig.json`
    /// `paths` and `baseUrl`, and packages in the project's `node_modules`. Dynamic
    /// `import()` is rejected. Modules are transpiled one by one and remembered by
    /// content, so reloading a project only transpiles the files that changed. A
    /// failed load keeps the previous version; see [`Engine`] for the `ctx.hot` state
    /// a replaced version hands over.
    pub fn load_project(
        &mut self,
        id: impl Into<ScriptId>,
        entry_path: impl AsRef<Path>,
    ) -> Result<(), VmError> {
        self.mount_project(id.into(), entry_path.as_ref())?
    }

    /// [`Engine::load_project`], keeping a failed load apart from a failed `dispose` of
    /// the version it replaced.
    fn mount_project(&mut self, id: ScriptId, entry_path: &Path) -> Result<Disposed, VmError> {
        let external_modules = self.registry.import_module_names()?;
        let project = self.transpiler.project(entry_path, &external_modules)?;
        self.install_host_modules()?;
        let graph_id = self.next_graph_id();
        let graph = self.module_store.insert_project(
            &project.entry_module_id,
            project.modules,
            graph_id,
        )?;
        let origin = ScriptOrigin {
            module_keys: project.module_keys,
            project: Some(ProjectFiles {
                entry_path: entry_path.to_path_buf(),
                watched: project.watched,
            }),
        };
        self.mount_graph(id, graph, origin)
    }

    /// Reloads, in load order, every project whose files changed since it was loaded,
    /// and reports which reloads succeeded and which failed. A reload whose replaced
    /// version's `ctx.hot.dispose` threw is listed both in `reloaded` and in
    /// `dispose_failed`.
    ///
    /// A project is checked through the size and modification time of its module
    /// files, their directories, its `tsconfig.json` and the `package.json` files its
    /// imports resolved through; nothing is read unless one of them changed. Inline
    /// scripts are never reloaded here. A failed reload keeps the previous version
    /// running and is reported once: the next report only lists it again after
    /// another change. Call it from the host loop, for example once per second
    /// during development; it starts no thread.
    pub fn reload_changed(&mut self) -> ReloadReport {
        let changed = self
            .scripts
            .iter()
            .filter_map(|(id, script)| {
                let project = script.origin.project.as_ref()?;
                project
                    .watched
                    .changed()
                    .then(|| (id.clone(), project.entry_path.clone()))
            })
            .collect::<Vec<_>>();

        let mut report = ReloadReport::default();
        for (id, entry_path) in changed {
            match self.mount_project(id.clone(), &entry_path) {
                Ok(disposed) => {
                    if let Err(error) = disposed {
                        report.dispose_failed.push((id.clone(), error));
                    }
                    report.reloaded.push(id);
                }
                Err(error) => {
                    self.restamp(&id);
                    report.failed.push((id, error));
                }
            }
        }
        report
    }

    /// Takes a project's files as they are now as seen, so a failed reload is reported
    /// once.
    fn restamp(&mut self, id: &str) {
        if let Some(project) = self
            .scripts
            .get_mut(id)
            .and_then(|script| script.origin.project.as_mut())
        {
            project.watched.restamp();
        }
    }

    /// QuickJS memory counters for the whole engine.
    pub fn memory_stats(&self) -> MemoryStats {
        memory_stats(self.runtime.memory_usage())
    }

    /// A handle that stops this engine's running JavaScript from any thread.
    pub fn interrupt_handle(&self) -> InterruptHandle {
        InterruptHandle::new(Arc::clone(&self.execution))
    }

    /// Unloads one script and releases its modules. Its `ctx.hot.dispose` callbacks run
    /// first; when one throws, the script is unloaded all the same and the error is
    /// returned.
    pub fn unload_script(&mut self, id: &str) -> Result<(), VmError> {
        let script = self
            .scripts
            .shift_remove(id)
            .ok_or_else(|| script_not_found(id))?;
        self.retire(script)?.map_err(|error| {
            hot_hook_error(
                error,
                &format!("`ctx.hot.dispose` of script `{id}` failed; the script is unloaded"),
            )
        })
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
            let value = self.await_returned(&ctx, returned, || {
                format!("function `{function}` of script `{script_id}`")
            })?;
            R::decode_js(&ctx, value)
                .catch(&ctx)
                .map_err(caught_js_error)
        });
        self.attribute_interrupt(self.settle(result))
    }

    /// Delivers one event to every handler registered for it, script by script in load
    /// order; returns the number of scripts that had at least one handler. A throwing
    /// handler does not stop the others: every handler runs, then the first error is
    /// returned. The payload is encoded once per script with handlers and shared by all
    /// of that script's handlers. Scripts that never registered a handler for `event`
    /// cost a set lookup.
    pub fn emit<P: JsEncode + ?Sized>(&self, event: &str, payload: &P) -> Result<usize, VmError> {
        self.dispatch(event, payload, |_, _, _| Ok(()))
    }

    /// Delivers one event like [`Engine::emit`] and returns what every handler returned,
    /// with the id of its script, in delivery order: scripts in load order, handlers in
    /// registration order. An `async` handler's Promise is awaited first, running
    /// script Promise jobs only. Every handler runs even after one fails, a throw, a
    /// rejection or a reply that does not decode as `R`; the first error is returned.
    /// Register the event with
    /// [`typed_request`](crate::InMemoryHostContractRegistry::typed_request) so the
    /// generated TypeScript types the handlers' reply.
    pub fn request<P: JsEncode + ?Sized, R: JsDecode>(
        &self,
        event: &str,
        payload: &P,
    ) -> Result<Vec<(&str, R)>, VmError> {
        let mut replies = Vec::new();
        self.dispatch(event, payload, |script_id, ctx, returned| {
            let value = self.await_returned(ctx, returned, || {
                format!("a `{event}` handler of script `{script_id}`")
            })?;
            let reply = R::decode_js(ctx, value)
                .catch(ctx)
                .map_err(caught_js_error)?;
            replies.push((script_id, reply));
            Ok(())
        })?;
        Ok(replies)
    }

    /// Runs every handler of `event` in load order, handing each returned value to
    /// `on_return`; see [`Engine::emit`].
    fn dispatch<'a, P: JsEncode + ?Sized>(
        &'a self,
        event: &str,
        payload: &P,
        mut on_return: impl for<'js> FnMut(&'a str, &Ctx<'js>, JsValue<'js>) -> Result<(), VmError>,
    ) -> Result<usize, VmError> {
        let _budget = self.budget();
        // Every script visited has at least one handler for the event.
        let mut delivered = 0;
        let mut first_error = None;
        for (script_id, script) in &self.scripts {
            let Some(handlers) = script.signals.events.handlers(event) else {
                continue;
            };
            delivered += 1;
            let outcome = script.context.with(|ctx| {
                deliver(&ctx, &handlers, payload, |returned| {
                    on_return(script_id, &ctx, returned)
                })
            });
            if let Err(error) = outcome {
                first_error.get_or_insert(error);
            }
        }
        if delivered == 0 {
            // No JavaScript ran, so there is no Promise job to settle.
            return Ok(0);
        }
        self.attribute_interrupt(self.settle(first_error.map_or(Ok(delivered), Err)))
    }

    /// Awaits a Promise a script returned to the host, from an export or a request
    /// handler, by running the job queue; `what` names the function in the error of a
    /// Promise that cannot settle. Its rejection is the error, so it is not also
    /// reported as unhandled.
    fn await_returned<'js>(
        &self,
        ctx: &Ctx<'js>,
        returned: JsValue<'js>,
        what: impl FnOnce() -> String,
    ) -> Result<JsValue<'js>, VmError> {
        let Some(promise) = returned.as_promise() else {
            return Ok(returned);
        };
        let settled = promise.finish::<JsValue<'_>>().catch(ctx);
        self.rejections.forget(ctx, &returned);
        settled.map_err(|error| match error {
            CaughtError::Error(rquickjs::Error::WouldBlock) => VmError::Execution {
                details: format!(
                    "{} returned a Promise that never settles: only script Promise jobs run on an Engine",
                    what()
                ),
            },
            error => caught_js_error(error),
        })
    }

    /// Runs every job left in the queue, for all scripts, then reports in order: the
    /// operation's own error, a job that threw, a Promise rejection nobody handled,
    /// with its locations pointing at the TypeScript source. Runs even when the
    /// operation failed, so nothing leaks into the next one.
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
        let value = result.map_err(|error| in_typescript(error, &self.module_store))?;
        match job_error.or(unhandled) {
            Some(error) => Err(in_typescript(error, &self.module_store)),
            None => Ok(value),
        }
    }

    /// Reports an operation that failed after an [`InterruptHandle`] request as
    /// [`VmError::Interrupted`] rather than as the QuickJS interrupt exception.
    fn attribute_interrupt<T>(&self, result: Result<T, VmError>) -> Result<T, VmError> {
        match result {
            Err(_) if self.execution.interrupt_requested() => Err(VmError::Interrupted),
            result => result,
        }
    }

    fn install_host_modules(&self) -> Result<(), VmError> {
        self.module_store
            .insert_host_modules(self.registry.import_modules()?)
    }

    fn next_graph_id(&mut self) -> u64 {
        let graph_id = self.next_graph_id;
        self.next_graph_id += 1;
        graph_id
    }

    /// Evaluates a freshly inserted graph, seeded with the `ctx.hot` state the loaded
    /// version saves, and swaps it in; on failure, the graph is removed and the
    /// previous version of the script stays loaded, not disposed.
    fn mount_graph(
        &mut self,
        id: ScriptId,
        graph: RuntimeModuleGraph,
        origin: ScriptOrigin,
    ) -> Result<Disposed, VmError> {
        let mounted = self
            .save_hot_data(&id)
            .and_then(|hot_data| self.mount(&id, &graph.entry_module_id, hot_data));
        match mounted {
            Ok((context, exports, signals)) => self.replace_script(
                id,
                EngineScript {
                    context,
                    exports,
                    signals,
                    module_ids: graph.module_ids,
                    origin,
                },
            ),
            Err(error) => {
                self.module_store.remove_modules(&graph.module_ids)?;
                Err(error)
            }
        }
    }

    /// Runs the loaded version's `ctx.hot.save` callback, when the script is loaded;
    /// its result becomes the next version's `ctx.hot.data`.
    fn save_hot_data(&self, id: &str) -> Result<Option<HotData>, VmError> {
        let Some(script) = self.scripts.get(id) else {
            return Ok(None);
        };
        let _budget = self.budget();
        let saved = script
            .context
            .with(|ctx| call_hot_hook(&ctx, HOT_SAVE).map(|data| Persistent::save(&ctx, data)));
        self.attribute_interrupt(self.settle(saved))
            .map(Some)
            .map_err(|error| {
                hot_hook_error(
                    error,
                    &format!(
                        "`ctx.hot.save` of script `{id}` failed; the previous version keeps running"
                    ),
                )
            })
    }

    fn mount(
        &self,
        script_id: &str,
        entry_module_id: &str,
        hot_data: Option<HotData>,
    ) -> Result<(Context, Persistent<Object<'static>>, ScriptSignals), VmError> {
        let _budget = self.budget();
        let context = Context::full(&self.runtime).map_err(js_error)?;
        let signals = ScriptSignals::default();
        let exports = context.with(|ctx| {
            self.install_native_functions(&ctx, &signals, script_id)?;
            install_hot_data(&ctx, hot_data)?;
            evaluate_script(&ctx, CONTEXT_PRELUDE)?;
            import_exports(&ctx, entry_module_id)
        });
        let exports = self.attribute_interrupt(self.settle(exports))?;
        Ok((context, exports, signals))
    }

    /// Installs the host functions and the `console` hook, bound to `script_id`, and the
    /// hooks through which the prelude reports handlers and timers to `signals`.
    fn install_native_functions(
        &self,
        ctx: &Ctx<'_>,
        signals: &ScriptSignals,
        script_id: &str,
    ) -> Result<(), VmError> {
        let functions = Object::new(ctx.clone()).map_err(js_error)?;
        self.registry
            .install_native_functions(&functions, script_id)
            .map_err(js_error)?;
        let globals = ctx.globals();
        globals
            .set(NATIVE_FUNCTIONS_GLOBAL, functions)
            .map_err(js_error)?;
        signals.events.install_hook(ctx)?;
        self.console.install(ctx, script_id, &self.module_store)?;
        install_timer_hooks(ctx, &self.timer_clock, &signals.next_timer)
    }

    /// Swaps `script` in, then retires the version it replaces, whose failed `dispose`
    /// does not undo the swap.
    fn replace_script(&mut self, id: ScriptId, script: EngineScript) -> Result<Disposed, VmError> {
        let Some(previous) = self.scripts.insert(id.clone(), script) else {
            return Ok(Ok(()));
        };
        Ok(self.retire(previous)?.map_err(|error| {
            hot_hook_error(
                error,
                &format!("`ctx.hot.dispose` of the previous version of script `{id}` failed; the new version is loaded"),
            )
        }))
    }

    /// Runs the `ctx.hot.dispose` callbacks of a version no longer in `scripts`, then
    /// releases its modules; the context drops with `script`.
    fn retire(&mut self, script: EngineScript) -> Result<Disposed, VmError> {
        let disposed = self.dispose(&script);
        self.module_store.remove_modules(&script.module_ids)?;
        self.forget_unused_modules();
        Ok(disposed)
    }

    /// Calls a retired version's `ctx.hot.dispose` callbacks, in registration order.
    fn dispose(&self, script: &EngineScript) -> Disposed {
        let _budget = self.budget();
        let disposed = script
            .context
            .with(|ctx| call_hot_hook(&ctx, HOT_DISPOSE).map(|_| ()));
        self.attribute_interrupt(self.settle(disposed))
    }

    /// Drops memoized modules and project states no loaded script uses anymore, so
    /// editing files during a long session does not grow them.
    fn forget_unused_modules(&mut self) {
        let modules = self
            .scripts
            .values()
            .flat_map(|script| &script.origin.module_keys)
            .map(String::as_str)
            .collect();
        let projects = self
            .scripts
            .values()
            .filter_map(|script| script.origin.project.as_ref())
            .map(|project| project.entry_path.as_path())
            .collect();
        self.transpiler.retain_used(&modules, &projects);
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

/// Hands the previous version's saved state to the prelude, which moves it to
/// `ctx.hot.data`.
fn install_hot_data(ctx: &Ctx<'_>, hot_data: Option<HotData>) -> Result<(), VmError> {
    let Some(hot_data) = hot_data else {
        return Ok(());
    };
    let data = hot_data.restore(ctx).map_err(js_error)?;
    ctx.globals().set(HOT_DATA_GLOBAL, data).map_err(js_error)
}

/// Calls one of the `ctx.hot` hooks the prelude locked on the context's global object.
fn call_hot_hook<'js>(ctx: &Ctx<'js>, hook: &str) -> Result<JsValue<'js>, VmError> {
    let hooks = ctx
        .globals()
        .get::<_, Object<'js>>(HOT_GLOBAL)
        .map_err(js_error)?;
    hooks
        .get::<_, Function<'js>>(hook)
        .map_err(js_error)?
        .call::<_, JsValue<'js>>(())
        .catch(ctx)
        .map_err(caught_js_error)
}

/// Reports a failed `ctx.hot` callback as [`VmError::Execution`] whose details start
/// with `outcome`, which says what became of the script.
fn hot_hook_error(error: VmError, outcome: &str) -> VmError {
    let details = match error {
        VmError::Execution { details } => details,
        error => error.to_string(),
    };
    VmError::Execution {
        details: format!("{outcome}: {details}"),
    }
}

fn script_not_found(id: &str) -> VmError {
    VmError::ScriptNotFound {
        script_id: id.to_owned(),
    }
}
