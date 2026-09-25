//! Script mounting, ESM graph installation, and unload.

use rquickjs::{CatchResultExt, Context, Ctx, Module};

use crate::compiler::CompiledModule;
use crate::error::VmError;
use crate::types::ScriptId;

use super::errors::{caught_js_error, js_error};
use super::host_bridge::{insert_host_import_modules, install_host_bridge};
use super::module_loader::RuntimeModuleGraph;
use super::render::{
    bootstrap_module_context_source, global_eval_options, list_subscriptions_source,
};
use super::script_store::LoadedScript;
use super::state::WorkerState;

pub(crate) fn load_script_into_runtime(
    state: &mut WorkerState,
    id: ScriptId,
    transpiled_js: String,
    entry_module_id: Option<String>,
    modules: Vec<CompiledModule>,
) -> Result<Vec<String>, VmError> {
    ensure_script_capacity(state, &id)?;
    let graph_id = state.next_module_graph_id();
    insert_host_import_modules(&state.module_store, &state.host_registry)?;
    let graph = install_modules(
        state,
        &id,
        transpiled_js,
        entry_module_id,
        modules,
        graph_id,
    )?;
    let prepared = create_script_context(state, &id, &graph.entry_module_id).and_then(|context| {
        collect_script_subscriptions(&context).map(|subscriptions| (context, subscriptions))
    });
    let (context, subscriptions) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            state.module_store.remove_modules(&graph.module_ids)?;
            return Err(error);
        }
    };
    remove_existing_script_modules(state, &id)?;
    insert_loaded_script(state, id, context, graph);
    Ok(subscriptions)
}

pub(crate) fn unload_loaded_script(
    state: &mut WorkerState,
    script_id: ScriptId,
) -> Result<(), VmError> {
    if let Some(script) = state.scripts.remove(&script_id) {
        state.module_store.remove_modules(&script.module_ids)?;
        return Ok(());
    }

    Err(VmError::ScriptNotFound { script_id })
}

fn ensure_script_capacity(state: &WorkerState, script_id: &str) -> Result<(), VmError> {
    if state.scripts.contains_key(script_id) {
        return Ok(());
    }

    if state.scripts.len() < state.options.max_scripts_per_worker {
        return Ok(());
    }

    Err(VmError::ScriptLimitReached {
        worker_id: state.worker_id,
        max_scripts: state.options.max_scripts_per_worker,
    })
}

fn remove_existing_script_modules(state: &mut WorkerState, script_id: &str) -> Result<(), VmError> {
    if let Some(script) = state.scripts.remove(script_id) {
        state.module_store.remove_modules(&script.module_ids)?;
    }
    Ok(())
}

fn install_modules(
    state: &WorkerState,
    script_id: &str,
    transpiled_js: String,
    entry_module_id: Option<String>,
    modules: Vec<CompiledModule>,
    graph_id: u64,
) -> Result<RuntimeModuleGraph, VmError> {
    if modules.is_empty() {
        return state
            .module_store
            .insert_inline(script_id, transpiled_js, graph_id);
    }

    let entry_module_id = entry_module_id.ok_or_else(|| VmError::Resolve {
        details: format!("project script has no entry module id: {script_id}"),
    })?;
    state
        .module_store
        .insert_project(&entry_module_id, modules, graph_id)
}

fn create_script_context(
    state: &WorkerState,
    script_id: &str,
    module_id: &str,
) -> Result<Context, VmError> {
    let context = Context::full(&state.runtime).map_err(js_error)?;
    install_host_bridge(&context, state.host_registry.clone())?;
    context.with(|ctx| bootstrap_module_context(ctx, script_id, module_id))?;
    Ok(context)
}

fn bootstrap_module_context(ctx: Ctx<'_>, script_id: &str, module_id: &str) -> Result<(), VmError> {
    evaluate_bootstrap_script(ctx.clone(), script_id)?;
    Module::import(&ctx, module_id)
        .and_then(|promise| promise.finish::<rquickjs::Object<'_>>())
        .catch(&ctx)
        .map_err(caught_js_error)?;
    Ok(())
}

fn evaluate_bootstrap_script(ctx: Ctx<'_>, script_id: &str) -> Result<(), VmError> {
    let source = bootstrap_module_context_source();
    ctx.eval_with_options::<(), _>(source, global_eval_options(script_id))
        .catch(&ctx)
        .map_err(caught_js_error)
}

fn insert_loaded_script(
    state: &mut WorkerState,
    id: ScriptId,
    context: Context,
    graph: RuntimeModuleGraph,
) {
    state.scripts.insert(
        id,
        LoadedScript {
            context,
            module_id: graph.entry_module_id,
            module_ids: graph.module_ids,
        },
    );
}

fn collect_script_subscriptions(context: &Context) -> Result<Vec<String>, VmError> {
    let result_json = context.with(|ctx| {
        ctx.eval::<String, _>(list_subscriptions_source())
            .catch(&ctx)
            .map_err(caught_js_error)
    })?;
    serde_json::from_str(&result_json).map_err(VmError::from)
}
