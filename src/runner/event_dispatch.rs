//! Event delivery into subscribed loaded scripts.

use rquickjs::CatchResultExt;
use serde_json::Value;

use crate::error::VmError;
use crate::types::ScriptId;

use super::errors::caught_js_error;
use super::render::emit_event_source;
use super::script_store::{LoadedScript, LoadedScriptMap};

pub(crate) fn emit_event_to_scripts(
    scripts: &LoadedScriptMap,
    target_script_ids: &[ScriptId],
    event_name: &str,
    payload: &Value,
) -> Result<usize, VmError> {
    if target_script_ids.is_empty() {
        return Ok(0);
    }

    let eval_source = build_emit_event_source(event_name, payload)?;
    let mut delivery_count = 0usize;

    // A target unloaded between routing and delivery no longer listens; skip it.
    for script in target_script_ids.iter().filter_map(|id| scripts.get(id)) {
        delivery_count += eval_emit_event(script, &eval_source)?;
    }

    Ok(delivery_count)
}

fn build_emit_event_source(event_name: &str, payload: &Value) -> Result<String, VmError> {
    let event_name_json = serde_json::to_string(event_name)?;
    let payload_json = serde_json::to_string(payload)?;
    Ok(emit_event_source(&event_name_json, &payload_json))
}

fn eval_emit_event(script: &LoadedScript, eval_source: &str) -> Result<usize, VmError> {
    script.context.with(|ctx| {
        ctx.eval::<usize, _>(eval_source)
            .catch(&ctx)
            .map_err(caught_js_error)
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use super::emit_event_to_scripts;
    use crate::config::VmOptions;
    use crate::runner::load::load_script_into_runtime;
    use crate::runner::state::WorkerState;

    #[test]
    fn emit_skips_targets_unloaded_after_routing() {
        let mut state = WorkerState::new(0, VmOptions::default(), Arc::default()).unwrap();
        load_script_into_runtime(
            &mut state,
            "listener".into(),
            "ctx.on(\"tick\", () => {});".into(),
            None,
            Vec::new(),
        )
        .unwrap();
        let targets = ["unloaded".to_owned(), "listener".to_owned()];

        let delivered = emit_event_to_scripts(&state.scripts, &targets, "tick", &json!(null));

        assert_eq!(delivered.unwrap(), 1);
    }
}
