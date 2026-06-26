//! Host event emission and callback delivery policy.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use serde_json::Value;

use crate::contract::{DeliveryMode, HostCallback};
use crate::error::VmError;
use crate::registry::{ActiveEventBinding, ActiveRuntimeRegistry};
use crate::types::{RuntimeExecutionLane, ScriptId, VmEvent, WorkerId};

use super::script_manager::ScriptManager;

impl ScriptManager {
    /// Emits one host event to every active script runtime.
    pub fn emit(&self, event_name: impl Into<String>, payload: Value) -> Result<usize, VmError> {
        self.emit_with_delivery(event_name, payload, DeliveryMode::Broadcast)
    }

    /// Emits one typed host callback contract to every active script runtime.
    pub fn emit_callback<T>(&self, payload: &T::Payload) -> Result<usize, VmError>
    where
        T: HostCallback,
    {
        let payload = serde_json::to_value(payload)?;
        self.emit_with_delivery(T::NAME, payload, T::delivery())
    }

    fn emit_with_delivery(
        &self,
        event_name: impl Into<String>,
        payload: Value,
        delivery: DeliveryMode,
    ) -> Result<usize, VmError> {
        let started_at = Instant::now();
        let result = self.emit_with_delivery_inner(event_name, payload, delivery);
        self.inner.metrics.observe_emit(started_at.elapsed());
        result
    }

    fn emit_with_delivery_inner(
        &self,
        event_name: impl Into<String>,
        payload: Value,
        delivery: DeliveryMode,
    ) -> Result<usize, VmError> {
        let event_name = event_name.into();
        let bindings = self
            .inner
            .active_runtime_registry
            .get_event_bindings(&event_name)?;
        let bindings = filter_bindings_for_delivery(bindings, delivery);
        let grouped = group_bindings_by_lane(bindings);
        let delivered_count = self.dispatch_grouped_event(&grouped, &event_name, &payload)?;

        self.inner.event_bus.publish(VmEvent::HostEventEmitted {
            event_name,
            delivered_count,
        });
        Ok(delivered_count)
    }

    #[cfg(feature = "tokio")]
    pub(crate) async fn emit_with_delivery_async(
        &self,
        event_name: impl Into<String>,
        payload: Value,
        delivery: DeliveryMode,
    ) -> Result<usize, VmError> {
        let started_at = Instant::now();
        let result = self
            .emit_with_delivery_inner_async(event_name, payload, delivery)
            .await;
        self.inner.metrics.observe_emit(started_at.elapsed());
        result
    }

    #[cfg(feature = "tokio")]
    async fn emit_with_delivery_inner_async(
        &self,
        event_name: impl Into<String>,
        payload: Value,
        delivery: DeliveryMode,
    ) -> Result<usize, VmError> {
        let event_name = event_name.into();
        let bindings = self
            .inner
            .active_runtime_registry
            .get_event_bindings(&event_name)?;
        let bindings = filter_bindings_for_delivery(bindings, delivery);
        let grouped = group_bindings_by_lane(bindings);
        let delivered_count = self
            .dispatch_grouped_event_async(&grouped, &event_name, &payload)
            .await?;

        self.inner.event_bus.publish(VmEvent::HostEventEmitted {
            event_name,
            delivered_count,
        });
        Ok(delivered_count)
    }

    fn dispatch_grouped_event(
        &self,
        grouped: &GroupedEventBindings,
        event_name: &str,
        payload: &Value,
    ) -> Result<usize, VmError> {
        let mut delivered_count = 0usize;
        for (worker_id, target_script_ids) in &grouped.sync {
            delivered_count += self.dispatch_emit_event(
                *worker_id,
                target_script_ids.clone(),
                event_name.to_owned(),
                payload.clone(),
            )?;
        }
        #[cfg(feature = "async-promise")]
        {
            for (worker_id, target_script_ids) in &grouped.async_ {
                delivered_count += self.inner.async_workers.emit_event(
                    *worker_id,
                    target_script_ids.clone(),
                    event_name.to_owned(),
                    payload.clone(),
                )?;
            }
        }
        Ok(delivered_count)
    }

    #[cfg(feature = "tokio")]
    async fn dispatch_grouped_event_async(
        &self,
        grouped: &GroupedEventBindings,
        event_name: &str,
        payload: &Value,
    ) -> Result<usize, VmError> {
        let mut delivered_count = 0usize;
        for (worker_id, target_script_ids) in &grouped.sync {
            let manager = self.clone();
            let worker_id = *worker_id;
            let target_script_ids = target_script_ids.clone();
            let event_name = event_name.to_owned();
            let payload = payload.clone();
            delivered_count += tokio::task::spawn_blocking(move || {
                manager.dispatch_emit_event(worker_id, target_script_ids, event_name, payload)
            })
            .await
            .map_err(|_| VmError::WorkerPanicked)??;
        }
        #[cfg(feature = "async-promise")]
        {
            for (worker_id, target_script_ids) in &grouped.async_ {
                delivered_count += self
                    .inner
                    .async_workers
                    .emit_event_async(
                        *worker_id,
                        target_script_ids.clone(),
                        event_name.to_owned(),
                        payload.clone(),
                    )
                    .await?;
            }
        }
        Ok(delivered_count)
    }
}

struct GroupedEventBindings {
    sync: HashMap<WorkerId, Vec<ScriptId>>,
    async_: HashMap<WorkerId, Vec<ScriptId>>,
}

fn group_bindings_by_lane(bindings: Arc<[ActiveEventBinding]>) -> GroupedEventBindings {
    let mut grouped = GroupedEventBindings {
        sync: HashMap::new(),
        async_: HashMap::new(),
    };
    for binding in bindings.iter() {
        target_group(&mut grouped, binding.execution_lane)
            .entry(binding.worker_id)
            .or_default()
            .push(binding.script_id.clone());
    }
    grouped
}

fn target_group(
    grouped: &mut GroupedEventBindings,
    lane: RuntimeExecutionLane,
) -> &mut HashMap<WorkerId, Vec<ScriptId>> {
    match lane {
        RuntimeExecutionLane::Sync => &mut grouped.sync,
        RuntimeExecutionLane::Async => &mut grouped.async_,
    }
}

fn filter_bindings_for_delivery(
    bindings: Arc<[ActiveEventBinding]>,
    delivery: DeliveryMode,
) -> Arc<[ActiveEventBinding]> {
    match delivery {
        DeliveryMode::Broadcast => bindings,
        DeliveryMode::First => bindings
            .first()
            .cloned()
            .map(|binding| Arc::<[ActiveEventBinding]>::from(vec![binding]))
            .unwrap_or_else(|| Arc::<[ActiveEventBinding]>::from([])),
    }
}
