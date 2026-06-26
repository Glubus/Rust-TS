use std::collections::HashMap;
use std::sync::Arc;

use arc_swap::ArcSwap;

use super::entry::ActiveRuntimeEntry;
use crate::types::{RuntimeExecutionLane, ScriptId, WorkerId};

/// One active script binding for a routed event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveEventBinding {
    /// Script receiving the event.
    pub script_id: ScriptId,
    /// Worker hosting the script.
    pub worker_id: WorkerId,
    /// Execution lane hosting the script.
    pub execution_lane: RuntimeExecutionLane,
}

type EventRouteTable = HashMap<String, Arc<[ActiveEventBinding]>>;

pub(super) struct EventRouteStore {
    routes: ArcSwap<EventRouteTable>,
}

impl Default for EventRouteStore {
    fn default() -> Self {
        Self {
            routes: ArcSwap::from_pointee(HashMap::new()),
        }
    }
}

impl EventRouteStore {
    pub(super) fn rebuild(&self, by_script_id: &HashMap<ScriptId, ActiveRuntimeEntry>) {
        let routes = collect_routes(by_script_id);
        self.routes.store(Arc::new(routes));
    }

    pub(super) fn bindings_for(&self, event_name: &str) -> Arc<[ActiveEventBinding]> {
        let routes = self.routes.load();
        routes
            .get(event_name)
            .cloned()
            .unwrap_or_else(|| Arc::<[ActiveEventBinding]>::from([]))
    }

    pub(super) fn binding_count(&self) -> usize {
        let routes = self.routes.load();
        routes.values().map(|bindings| bindings.len()).sum()
    }

    pub(super) fn snapshot(&self) -> Vec<(String, Vec<ActiveEventBinding>)> {
        let routes = self.routes.load();
        let mut snapshot = routes
            .iter()
            .map(|(event_name, bindings)| (event_name.clone(), bindings.to_vec()))
            .collect::<Vec<_>>();
        snapshot.sort_by(|left, right| left.0.cmp(&right.0));
        snapshot
    }
}

fn collect_routes(by_script_id: &HashMap<ScriptId, ActiveRuntimeEntry>) -> EventRouteTable {
    let mut routes = HashMap::<String, Vec<ActiveEventBinding>>::new();

    for (script_id, entry) in by_script_id {
        collect_script_routes(&mut routes, script_id, entry);
    }

    routes
        .into_iter()
        .map(|(event_name, bindings)| (event_name, Arc::<[ActiveEventBinding]>::from(bindings)))
        .collect()
}

fn collect_script_routes(
    routes: &mut HashMap<String, Vec<ActiveEventBinding>>,
    script_id: &str,
    entry: &ActiveRuntimeEntry,
) {
    for event_name in &entry.subscriptions {
        routes
            .entry(event_name.clone())
            .or_default()
            .push(ActiveEventBinding {
                script_id: script_id.to_owned(),
                worker_id: entry.worker_id,
                execution_lane: entry.execution_lane,
            });
    }
}
