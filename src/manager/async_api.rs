//! Optional Tokio async façade over the synchronous manager API.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use super::script_manager::ScriptManager;
use crate::contract::{DeliveryMode, HostCallback};
use crate::error::VmError;
use crate::types::{ScriptId, ScriptRetentionPolicy, ScriptSnapshot};

impl ScriptManager {
    /// Loads or replaces one TypeScript script on a blocking Tokio task.
    pub async fn load_script_async(
        &self,
        id: impl Into<ScriptId> + Send + 'static,
        source: impl Into<String> + Send + 'static,
    ) -> Result<ScriptSnapshot, VmError> {
        let manager = self.clone();
        let id = id.into();
        let source = source.into();
        spawn_manager_task(move || manager.load_script(id, source)).await
    }

    /// Loads or replaces one script with an explicit retention policy.
    pub async fn load_script_with_policy_async(
        &self,
        id: impl Into<ScriptId> + Send + 'static,
        source: impl Into<String> + Send + 'static,
        policy: ScriptRetentionPolicy,
    ) -> Result<ScriptSnapshot, VmError> {
        let manager = self.clone();
        let id = id.into();
        let source = source.into();
        spawn_manager_task(move || manager.load_script_with_policy(id, source, policy)).await
    }

    /// Loads one multi-file TypeScript project on a blocking Tokio task.
    pub async fn load_script_project_async(
        &self,
        id: impl Into<ScriptId> + Send + 'static,
        entry_path: impl AsRef<Path> + Send + 'static,
    ) -> Result<ScriptSnapshot, VmError> {
        let manager = self.clone();
        let id = id.into();
        let entry_path = entry_path.as_ref().to_path_buf();
        spawn_manager_task(move || manager.load_script_project(id, entry_path)).await
    }

    /// Calls one exported function on a blocking Tokio task.
    pub async fn call_function_async(
        &self,
        script_id: impl Into<ScriptId> + Send + 'static,
        function_name: impl Into<String> + Send + 'static,
        args: Vec<Value>,
    ) -> Result<Value, VmError> {
        let manager = self.clone();
        let script_id = script_id.into();
        let function_name = function_name.into();
        spawn_manager_task(move || manager.call_function(script_id, function_name, args)).await
    }

    /// Loads one script, calls one exported function, then demounts it.
    pub async fn call_function_once_async(
        &self,
        source: impl Into<String> + Send + 'static,
        function_name: impl Into<String> + Send + 'static,
        args: Vec<Value>,
    ) -> Result<Value, VmError> {
        let manager = self.clone();
        let source = source.into();
        let function_name = function_name.into();
        spawn_manager_task(move || manager.call_function_once(source, function_name, args)).await
    }

    /// Loads one project, calls one exported function, then demounts it.
    pub async fn call_function_once_project_async(
        &self,
        entry_path: impl AsRef<Path> + Send + 'static,
        function_name: impl Into<String> + Send + 'static,
        args: Vec<Value>,
    ) -> Result<Value, VmError> {
        let manager = self.clone();
        let entry_path: PathBuf = entry_path.as_ref().to_path_buf();
        let function_name = function_name.into();
        spawn_manager_task(move || {
            manager.call_function_once_project(entry_path, function_name, args)
        })
        .await
    }

    /// Emits one host event on a blocking Tokio task.
    pub async fn emit_async(
        &self,
        event_name: impl Into<String> + Send + 'static,
        payload: Value,
    ) -> Result<usize, VmError> {
        self.emit_with_delivery_async(event_name, payload, DeliveryMode::Broadcast)
            .await
    }

    /// Emits one typed host callback on a blocking Tokio task.
    pub async fn emit_callback_async<T>(&self, payload: T::Payload) -> Result<usize, VmError>
    where
        T: HostCallback + Send + Sync + 'static,
        T::Payload: Serialize + Send + 'static,
    {
        let payload = serde_json::to_value(payload)?;
        self.emit_with_delivery_async(T::NAME, payload, T::delivery())
            .await
    }

    /// Unloads one script on a blocking Tokio task.
    pub async fn unload_script_async(
        &self,
        script_id: impl Into<ScriptId> + Send + 'static,
    ) -> Result<(), VmError> {
        let manager = self.clone();
        let script_id = script_id.into();
        spawn_manager_task(move || manager.unload_script(script_id)).await
    }
}

async fn spawn_manager_task<T>(
    task: impl FnOnce() -> Result<T, VmError> + Send + 'static,
) -> Result<T, VmError>
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(task)
        .await
        .map_err(|_| VmError::WorkerPanicked)?
}
