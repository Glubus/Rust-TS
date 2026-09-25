//! Exported function invocation inside loaded scripts, shared by the sync and async lanes.

use rquickjs::{CatchResultExt, CaughtError, Promise};
use serde_json::Value;

use crate::error::VmError;

use super::errors::caught_js_error_details;
use super::render::function_call_source;
use super::script_store::{LoadedScript, LoadedScriptMap, get_loaded_script};

/// Calls one export on the synchronous lane. An async export settles by draining the
/// QuickJS job queue within the current execution budget.
pub(crate) fn call_script_function(
    scripts: &LoadedScriptMap,
    script_id: &str,
    function_name: &str,
    args: &[Value],
) -> Result<Value, VmError> {
    let script = get_loaded_script(scripts, script_id)?;
    let eval_source = build_function_call_source(&script.module_id, function_name, args)?;
    let result_json = eval_function_call(script, eval_source, script_id, function_name)?;
    deserialize_function_result(&result_json)
}

/// Script source whose Promise settles to the JSON text of the export's result.
pub(crate) fn build_function_call_source(
    module_id: &str,
    function_name: &str,
    args: &[Value],
) -> Result<String, VmError> {
    let module_id_json = serde_json::to_string(module_id)?;
    let function_name_json = serde_json::to_string(function_name)?;
    let args_json = serde_json::to_string(args)?;
    Ok(function_call_source(
        &module_id_json,
        &function_name_json,
        &args_json,
    ))
}

fn eval_function_call(
    script: &LoadedScript,
    eval_source: String,
    script_id: &str,
    function_name: &str,
) -> Result<String, VmError> {
    script.context.with(|ctx| {
        ctx.eval::<Promise<'_>, _>(eval_source)
            .and_then(|promise| promise.finish::<String>())
            .catch(&ctx)
            .map_err(|error| map_function_call_error(error, script_id, function_name))
    })
}

pub(crate) fn map_function_call_error(
    error: CaughtError<'_>,
    script_id: &str,
    function_name: &str,
) -> VmError {
    if matches!(error, CaughtError::Error(rquickjs::Error::WouldBlock)) {
        return VmError::Execution {
            details: format!(
                "function `{function_name}` of script `{script_id}` returned a Promise that never settles on the synchronous worker lane"
            ),
        };
    }

    let details = caught_js_error_details(&error);
    if details.contains("missing function:") {
        VmError::FunctionNotFound {
            script_id: script_id.to_owned(),
            function_name: function_name.to_owned(),
        }
    } else {
        VmError::Execution { details }
    }
}

pub(crate) fn deserialize_function_result(result_json: &str) -> Result<Value, VmError> {
    serde_json::from_str(result_json).map_err(VmError::from)
}
