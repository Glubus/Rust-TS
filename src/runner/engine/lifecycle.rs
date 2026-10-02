//! Loading, reloading and unloading scripts, and the `ctx.hot` hand-over between
//! versions.

use std::cell::RefCell;
use std::path::Path;

use rquickjs::{
    CatchResultExt, Context, Ctx, Function, Module, Object, Persistent, Runtime, Value as JsValue,
    WriteOptions, qjs,
};

use crate::config::ScriptBuiltins;
use crate::error::VmError;
use crate::types::{ReloadReport, ScriptId};

use super::super::errors::{caught_js_error, js_error};
use super::super::host_promises::HostPromises;
use super::super::module_loader::RuntimeModuleGraph;
use super::super::timers::install_timer_hooks;
use super::{
    Disposed, Engine, EngineScript, HotData, ProjectFiles, ScriptOrigin, ScriptSignals,
    script_not_found,
};

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
/// A module, so that its compiled form can be kept and loaded into every context.
const CONTEXT_PRELUDE: &str = include_str!("../../../assets/context_prelude.js");
const PRELUDE_MODULE: &str = "rustts:prelude";

impl Engine {
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
            Ok((context, exports, signals, host_promises)) => self.replace_script(
                id,
                EngineScript {
                    context,
                    tasks: RefCell::new(Vec::new()),
                    request_guards: RefCell::new(Vec::new()),
                    exports,
                    signals,
                    host_promises,
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
    ) -> Result<
        (
            Context,
            Persistent<Object<'static>>,
            ScriptSignals,
            HostPromises,
        ),
        VmError,
    > {
        let _budget = self.budget();
        let context = new_context(&self.runtime, self.builtins)?;
        let signals = ScriptSignals::default();
        let host_promises = HostPromises::default();
        let exports = context.with(|ctx| {
            self.install_native_functions(&ctx, &signals, script_id, &host_promises)?;
            install_hot_data(&ctx, hot_data)?;
            self.run_prelude(&ctx)?;
            import_exports(&ctx, entry_module_id)
        });
        let exports = self.attribute_interrupt(self.settle(exports))?;
        Ok((context, exports, signals, host_promises))
    }

    /// Runs the context prelude in `ctx`. The first call compiles it and keeps the
    /// bytecode; every later context loads that instead of parsing and compiling the
    /// source again, which was most of the cost of mounting a script.
    fn run_prelude(&self, ctx: &Ctx<'_>) -> Result<(), VmError> {
        let declared =
            match self.prelude.get() {
                // SAFETY: QuickJS does not verify bytecode. These bytes were written by this
                // process, from the crate's own prelude, by the same QuickJS build.
                Some(bytecode) => unsafe { Module::load(ctx.clone(), bytecode) },
                None => {
                    let declared = Module::declare(ctx.clone(), PRELUDE_MODULE, CONTEXT_PRELUDE);
                    if let Ok(bytecode) = declared.as_ref().map_err(|_| ()).and_then(|declared| {
                        declared.write(WriteOptions::default()).map_err(|_| ())
                    }) {
                        // Only this thread sets it, once: the first mount.
                        let _ = self.prelude.set(bytecode.into_boxed_slice());
                    }
                    declared
                }
            }
            .catch(ctx)
            .map_err(caught_js_error)?;
        let (_, evaluated) = declared.eval().catch(ctx).map_err(caught_js_error)?;
        evaluated.finish::<()>().catch(ctx).map_err(caught_js_error)
    }

    /// Installs the host functions and the `console` hook, bound to `script_id`, and the
    /// hooks through which the prelude reports handlers and timers to `signals`.
    fn install_native_functions(
        &self,
        ctx: &Ctx<'_>,
        signals: &ScriptSignals,
        script_id: &str,
        host_promises: &HostPromises,
    ) -> Result<(), VmError> {
        let functions = Object::new(ctx.clone()).map_err(js_error)?;
        self.registry
            .install_native_functions(&functions, script_id, host_promises)
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
        for guard in script.request_guards.borrow_mut().drain(..) {
            guard.cancel();
        }
        self.active_tasks
            .set(self.active_tasks.get() - script.tasks.borrow().len());
        script.tasks.borrow_mut().clear();
        let disposed = self.dispose(&script);
        script.host_promises.cancel();
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

/// A context with the core, the built-ins RustTS needs and the optional ones `builtins`
/// asks for. All of them is `Context::full`; fewer adds the same intrinsics one by one,
/// in the order QuickJS's `JS_NewContext` does.
fn new_context(runtime: &Runtime, builtins: ScriptBuiltins) -> Result<Context, VmError> {
    if builtins == ScriptBuiltins::ALL {
        return Context::full(runtime).map_err(js_error);
    }
    let context = Context::custom::<()>(runtime).map_err(js_error)?;
    let failed = context.with(|ctx| {
        let raw = ctx.as_raw().as_ptr();
        let failures = |enabled: bool, add: unsafe extern "C" fn(*mut qjs::JSContext) -> i32| {
            // SAFETY: `raw` is the live context `ctx` wraps. Each intrinsic is added once,
            // right after creation and before any script runs, as `JS_NewContext` does.
            enabled && unsafe { add(raw) } != 0
        };
        // `eval` compiles modules and the prelude, JSON backs `console`, `Map` backs the
        // prelude and `Promise` backs `async`: always on.
        failures(builtins.date, qjs::JS_AddIntrinsicDate)
            || failures(true, qjs::JS_AddIntrinsicEval)
            || failures(builtins.regexp, qjs::JS_AddIntrinsicRegExp)
            || failures(true, qjs::JS_AddIntrinsicJSON)
            || failures(builtins.proxy, qjs::JS_AddIntrinsicProxy)
            || failures(true, qjs::JS_AddIntrinsicMapSet)
            || failures(builtins.typed_arrays, qjs::JS_AddIntrinsicTypedArrays)
            || failures(true, qjs::JS_AddIntrinsicPromise)
            || failures(builtins.weak_ref, qjs::JS_AddIntrinsicWeakRef)
            || failures(builtins.web, qjs::JS_AddIntrinsicAToB)
            || failures(builtins.web, qjs::JS_AddPerformance)
    });
    if failed {
        Err(VmError::Execution {
            details: String::from("out of memory while creating a script context"),
        })
    } else {
        Ok(context)
    }
}
