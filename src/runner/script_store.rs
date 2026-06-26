//! Per-worker loaded script store.

use std::collections::HashMap;

use rquickjs::Context;

use crate::error::VmError;
use crate::types::ScriptId;

#[derive(Clone)]
pub(crate) struct LoadedScript {
    pub(crate) context: Context,
    pub(crate) module_id: String,
    pub(crate) module_ids: Vec<String>,
}

pub(crate) type LoadedScriptMap = HashMap<ScriptId, LoadedScript>;

pub(crate) fn get_loaded_script<'a>(
    scripts: &'a LoadedScriptMap,
    script_id: &str,
) -> Result<&'a LoadedScript, VmError> {
    scripts
        .get(script_id)
        .ok_or_else(|| VmError::ScriptNotFound {
            script_id: script_id.to_owned(),
        })
}
