//! Event delivery into subscribed loaded scripts.

use rquickjs::CatchResultExt;
use serde_json::Value;

use crate::error::VmError;
use crate::types::ScriptId;

use super::errors::caught_js_error;
use super::render::emit_event_source;
use super::script_store::{LoadedScript, LoadedScriptMap, get_loaded_script};

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

    for script_id in target_script_ids {
        let script = get_loaded_script(scripts, script_id)?;
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
