//! Manager-level exported function calls.

use std::path::Path;
use std::time::Instant;

use serde_json::Value;

use crate::error::VmError;
use crate::registry::ActiveRuntimeRegistry;
use crate::types::{ScriptId, ScriptRetentionPolicy, VmEvent};

use super::script_manager::ScriptManager;

impl ScriptManager {
    /// Calls one exported function and converts the result through JSON.
    pub fn call_function(
        &self,
        script_id: impl Into<ScriptId>,
        function_name: impl Into<String>,
        args: Vec<Value>,
    ) -> Result<Value, VmError> {
        let started_at = Instant::now();
        let script_id = script_id.into();
        let function_name = function_name.into();
        let result = self.call_function_inner(script_id, function_name, args);
        self.inner.metrics.observe_call(started_at.elapsed());
        result
    }

    fn call_function_inner(
        &self,
        script_id: ScriptId,
        function_name: String,
        args: Vec<Value>,
    ) -> Result<Value, VmError> {
        let worker_id = self.lookup_worker_for_script(&script_id)?;
        self.inner
            .active_runtime_registry
            .begin_execution(&script_id)?;
        let result =
            self.dispatch_call_function(worker_id, script_id.clone(), function_name.clone(), args);
        let should_demount = self
            .inner
            .active_runtime_registry
            .end_execution(&script_id)?;

        match result {
            Ok(result) => self.handle_successful_call(
                worker_id,
                script_id,
                function_name,
                result,
                should_demount,
            ),
            Err(error) => {
                if should_demount {
                    self.demount_script(worker_id, &script_id)?;
                }
                Err(error)
            }
        }
    }

    /// Loads one script as a oneshot, calls one exported function, then demounts automatically.
    pub fn call_function_once(
        &self,
        source: impl Into<String>,
        function_name: impl Into<String>,
        args: Vec<Value>,
    ) -> Result<Value, VmError> {
        let source = source.into();
        let function_name = function_name.into();
        let script_id = self.next_oneshot_script_id();

        self.load_script_with_policy(
            script_id.clone(),
            source,
            ScriptRetentionPolicy::DemountWhenIdle,
        )?;
        self.call_function(script_id, function_name, args)
    }

    /// Loads one project as a oneshot, calls one exported function, then demounts automatically.
    ///
    /// Version 0 supports static ESM graphs with local imports, tsconfig aliases, and
    /// package imports resolved from project-local `node_modules`.
    pub fn call_function_once_project(
        &self,
        entry_path: impl AsRef<Path>,
        function_name: impl Into<String>,
        args: Vec<Value>,
    ) -> Result<Value, VmError> {
        let function_name = function_name.into();
        let script_id = self.next_oneshot_script_id();

        self.load_script_project_with_policy(
            script_id.clone(),
            entry_path,
            ScriptRetentionPolicy::DemountWhenIdle,
        )?;
        self.call_function(script_id, function_name, args)
    }

    fn handle_successful_call(
        &self,
        worker_id: usize,
        script_id: ScriptId,
        function_name: String,
        result: Value,
        should_demount: bool,
    ) -> Result<Value, VmError> {
        self.inner.event_bus.publish(VmEvent::FunctionCalled {
            worker_id,
            script_id: script_id.clone(),
            function_name,
            result: result.clone(),
        });
        if should_demount {
            self.demount_script(worker_id, &script_id)?;
        }
        Ok(result)
    }
}
