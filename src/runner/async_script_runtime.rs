//! Experimental async QuickJS script runtime.

use std::sync::Arc;

use rquickjs::{AsyncContext, AsyncRuntime, CatchResultExt, Ctx, Module, Object, Promise};
use serde_json::Value;

use crate::compiler::CompiledModule;
use crate::config::VmOptions;
use crate::error::VmError;
use crate::registry::InMemoryHostContractRegistry;
use crate::types::{ScriptId, VmQuickJsMemoryStats};

use super::async_host_bridge::install_async_host_bridge;
use super::errors::{caught_js_error, js_error};
use super::host_bridge::insert_host_import_modules;
use super::invocation::{
    build_function_call_source, deserialize_function_result, map_function_call_error,
};
use super::memory::quickjs_memory_stats;
use super::module_loader::{
    MemoryModuleLoader, MemoryModuleResolver, RuntimeModuleGraph, WorkerModuleStore,
};
use super::render::{
    async_emit_event_source, bootstrap_module_context_source, global_eval_options,
    list_subscriptions_source,
};

/// Experimental async QuickJS runtime for Promise-aware host functions.
///
/// This type is intentionally low-level: it expects JavaScript that is already suitable
/// for QuickJS ESM execution. The main [`crate::RustTs`] control plane still owns normal
/// compile/cache/lifecycle orchestration.
pub struct AsyncScriptRuntime {
    control: Arc<super::execution::ExecutionControl>,
    budget: std::time::Duration,
    runtime: AsyncRuntime,
    module_store: WorkerModuleStore,
    host_registry: Arc<InMemoryHostContractRegistry>,
    next_module_graph_id: u64,
}

/// Script mounted inside an [`AsyncScriptRuntime`].
pub struct AsyncLoadedScript {
    control: Arc<super::execution::ExecutionControl>,
    budget: std::time::Duration,
    context: AsyncContext,
    module_store: WorkerModuleStore,
    module_id: String,
    module_ids: Vec<String>,
}

impl AsyncScriptRuntime {
    /// Creates one async runtime with an in-memory ESM loader and async host bridge.
    pub async fn new(
        options: &VmOptions,
        host_registry: Arc<InMemoryHostContractRegistry>,
    ) -> Result<Self, VmError> {
        Self::new_with_control(options, host_registry, Arc::default()).await
    }

    pub(crate) async fn new_with_control(
        options: &VmOptions,
        host_registry: Arc<InMemoryHostContractRegistry>,
        control: Arc<super::execution::ExecutionControl>,
    ) -> Result<Self, VmError> {
        let runtime = AsyncRuntime::new().map_err(js_error)?;
        let interrupt = control.clone();
        runtime
            .set_interrupt_handler(Some(Box::new(move || interrupt.interrupted())))
            .await;
        runtime.set_memory_limit(options.memory_limit_bytes).await;
        runtime
            .set_max_stack_size(options.max_stack_size_bytes)
            .await;

        let module_store = WorkerModuleStore::default();
        runtime
            .set_loader(
                MemoryModuleResolver::new(module_store.clone()),
                MemoryModuleLoader::new(module_store.clone()),
            )
            .await;

        Ok(Self {
            control,
            budget: options.execution_timeout,
            runtime,
            module_store,
            host_registry,
            next_module_graph_id: 0,
        })
    }

    /// Loads one inline ESM script into a new async QuickJS context.
    pub async fn load_inline_script(
        &mut self,
        id: impl Into<ScriptId>,
        transpiled_js: impl Into<String>,
    ) -> Result<(AsyncLoadedScript, Vec<String>), VmError> {
        self.load_script(id.into(), transpiled_js.into(), None, Vec::new())
            .await
    }

    pub(crate) async fn load_script(
        &mut self,
        id: ScriptId,
        transpiled_js: String,
        entry_module_id: Option<String>,
        modules: Vec<CompiledModule>,
    ) -> Result<(AsyncLoadedScript, Vec<String>), VmError> {
        let graph = self.install_modules(&id, transpiled_js, entry_module_id, modules)?;
        let mut cleanup = PendingGraph {
            store: self.module_store.clone(),
            modules: graph.module_ids.clone(),
        };
        let result = self
            .control
            .run_async(self.budget, async {
                let context = self
                    .create_script_context(&id, &graph.entry_module_id)
                    .await?;
                let subscriptions = collect_script_subscriptions(&context).await?;
                Ok((context, subscriptions))
            })
            .await;
        let (context, subscriptions) = result?;
        cleanup.modules.clear();
        Ok((
            build_loaded_script(
                context,
                self.module_store.clone(),
                graph,
                self.control.clone(),
                self.budget,
            ),
            subscriptions,
        ))
    }

    fn install_modules(
        &mut self,
        script_id: &str,
        transpiled_js: String,
        entry_module_id: Option<String>,
        modules: Vec<CompiledModule>,
    ) -> Result<RuntimeModuleGraph, VmError> {
        let graph_id = self.next_module_graph_id();
        insert_host_import_modules(&self.module_store, &self.host_registry)?;
        if modules.is_empty() {
            return self
                .module_store
                .insert_inline(script_id, transpiled_js, graph_id);
        }

        let entry_module_id = entry_module_id.ok_or_else(|| VmError::Resolve {
            details: format!("project script has no entry module id: {script_id}"),
        })?;
        self.module_store
            .insert_project(&entry_module_id, modules, graph_id)
    }

    async fn create_script_context(
        &self,
        script_id: &str,
        module_id: &str,
    ) -> Result<AsyncContext, VmError> {
        let context = AsyncContext::full(&self.runtime).await.map_err(js_error)?;
        install_async_host_bridge(&context, self.host_registry.clone()).await?;
        bootstrap_module_context(&context, script_id, module_id).await?;
        Ok(context)
    }

    fn next_module_graph_id(&mut self) -> u64 {
        let graph_id = self.next_module_graph_id;
        self.next_module_graph_id += 1;
        graph_id
    }

    pub(crate) async fn quickjs_memory_stats(&self) -> VmQuickJsMemoryStats {
        quickjs_memory_stats(self.runtime.memory_usage().await)
    }
}

impl AsyncLoadedScript {
    /// Calls an exported function and awaits the JavaScript result.
    pub async fn call_function(
        &self,
        script_id: &str,
        function_name: &str,
        args: &[Value],
    ) -> Result<Value, VmError> {
        let eval_source = build_function_call_source(&self.module_id, function_name, args)?;
        let result_json = self
            .control
            .run_async(
                self.budget,
                eval_async_function_call(&self.context, eval_source, script_id, function_name),
            )
            .await?;
        deserialize_function_result(&result_json)
    }

    pub(crate) async fn emit_event(
        &self,
        event_name: &str,
        payload: &Value,
    ) -> Result<usize, VmError> {
        let eval_source = build_async_emit_event_source(event_name, payload)?;
        self.control
            .run_async(
                self.budget,
                eval_async_emit_event(&self.context, eval_source),
            )
            .await
    }

    /// Returns runtime-scoped module IDs mounted for this script.
    #[must_use]
    pub fn module_ids(&self) -> &[String] {
        &self.module_ids
    }
}

fn build_async_emit_event_source(event_name: &str, payload: &Value) -> Result<String, VmError> {
    let event_name_json = serde_json::to_string(event_name)?;
    let payload_json = serde_json::to_string(payload)?;
    Ok(async_emit_event_source(&event_name_json, &payload_json))
}

async fn eval_async_emit_event(
    context: &AsyncContext,
    eval_source: String,
) -> Result<usize, VmError> {
    context
        .async_with(async |ctx| {
            ctx.eval::<Promise<'_>, _>(eval_source)
                .catch(&ctx)
                .map_err(caught_js_error)?
                .into_future::<usize>()
                .await
                .catch(&ctx)
                .map_err(caught_js_error)
        })
        .await
}

async fn bootstrap_module_context(
    context: &AsyncContext,
    script_id: &str,
    module_id: &str,
) -> Result<(), VmError> {
    context
        .async_with(async |ctx| {
            evaluate_bootstrap_script(ctx.clone(), script_id)?;
            import_entry_module(ctx, module_id).await
        })
        .await
}

fn evaluate_bootstrap_script(ctx: Ctx<'_>, script_id: &str) -> Result<(), VmError> {
    let source = bootstrap_module_context_source();
    ctx.eval_with_options::<(), _>(source, global_eval_options(script_id))
        .catch(&ctx)
        .map_err(caught_js_error)
}

async fn import_entry_module(ctx: Ctx<'_>, module_id: &str) -> Result<(), VmError> {
    Module::import(&ctx, module_id)
        .catch(&ctx)
        .map_err(caught_js_error)?
        .into_future::<Object<'_>>()
        .await
        .catch(&ctx)
        .map_err(caught_js_error)?;
    Ok(())
}

fn build_loaded_script(
    context: AsyncContext,
    module_store: WorkerModuleStore,
    graph: RuntimeModuleGraph,
    control: Arc<super::execution::ExecutionControl>,
    budget: std::time::Duration,
) -> AsyncLoadedScript {
    AsyncLoadedScript {
        control,
        budget,
        context,
        module_store,
        module_id: graph.entry_module_id,
        module_ids: graph.module_ids,
    }
}

struct PendingGraph {
    store: WorkerModuleStore,
    modules: Vec<String>,
}

impl Drop for PendingGraph {
    fn drop(&mut self) {
        if !self.modules.is_empty() {
            let _ = self.store.remove_modules(&self.modules);
        }
    }
}

impl Drop for AsyncLoadedScript {
    fn drop(&mut self) {
        let _ = self.module_store.remove_modules(&self.module_ids);
    }
}

async fn collect_script_subscriptions(context: &AsyncContext) -> Result<Vec<String>, VmError> {
    let result_json = context
        .async_with(async |ctx| {
            ctx.eval::<String, _>(list_subscriptions_source())
                .catch(&ctx)
                .map_err(caught_js_error)
        })
        .await?;
    serde_json::from_str(&result_json).map_err(VmError::from)
}

async fn eval_async_function_call(
    context: &AsyncContext,
    eval_source: String,
    script_id: &str,
    function_name: &str,
) -> Result<String, VmError> {
    context
        .async_with(async |ctx| {
            ctx.eval::<Promise<'_>, _>(eval_source)
                .catch(&ctx)
                .map_err(|error| map_function_call_error(error, script_id, function_name))?
                .into_future::<String>()
                .await
                .catch(&ctx)
                .map_err(|error| map_function_call_error(error, script_id, function_name))
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{AsyncHostFunction, HostContract, HostContractKind, Schema, TsType};
    use serde_json::json;

    const ASYNC_SCRIPT: &str = r#"
export async function lookup(id) {
  const userName = await user.find(id);
  return { userName };
}
"#;

    struct AsyncFindUser;

    impl HostContract for AsyncFindUser {
        const NAME: &'static str = "user.find";

        fn schema() -> Schema {
            Schema::typed("FindUserInput", TsType::Number)
        }

        fn kind() -> HostContractKind {
            HostContractKind::Function
        }
    }

    impl AsyncHostFunction for AsyncFindUser {
        type Future = std::future::Ready<Result<Self::Output, VmError>>;
        type Input = u64;
        type Output = String;

        fn output_schema() -> Schema {
            Schema::typed("FindUserOutput", TsType::String)
        }

        fn call_async(input: Self::Input) -> Self::Future {
            std::future::ready(Ok(format!("async-user-{input}")))
        }
    }

    #[test]
    fn async_runtime_loads_esm_and_awaits_host_promise_in_export() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let host_registry = Arc::new(InMemoryHostContractRegistry::new());
            host_registry
                .async_promise_function::<AsyncFindUser>()
                .expect("register async promise host function");

            let options = VmOptions::default();
            let mut runtime = AsyncScriptRuntime::new(&options, host_registry)
                .await
                .expect("create async script runtime");
            let (script, subscriptions) = runtime
                .load_inline_script("async-script", ASYNC_SCRIPT)
                .await
                .expect("load async script");

            let result = script
                .call_function("async-script", "lookup", &[json!(42)])
                .await
                .expect("call async export");

            assert!(subscriptions.is_empty());
            assert_eq!(script.module_ids().len(), 1);
            assert_eq!(result, json!({ "userName": "async-user-42" }));
        });
    }
}
