use std::sync::Arc;

use super::{ActiveEventBinding, ActiveRuntimeRegistry, InMemoryActiveRuntimeRegistry};
use crate::types::{RuntimeExecutionLane, ScriptRetentionPolicy};

#[test]
fn bind_then_get_worker_returns_bound_worker() {
    let registry = InMemoryActiveRuntimeRegistry::new();

    registry
        .bind_script(String::from("alpha"), 2, ScriptRetentionPolicy::KeepMounted)
        .unwrap();

    assert_eq!(registry.get_worker("alpha").unwrap(), Some(2));
}

#[test]
fn unbind_clears_binding() {
    let registry = InMemoryActiveRuntimeRegistry::new();

    registry
        .bind_script(String::from("alpha"), 2, ScriptRetentionPolicy::KeepMounted)
        .unwrap();
    let demountable = registry.unbind_script("alpha").unwrap();

    assert_eq!(registry.get_worker("alpha").unwrap(), None);
    assert!(demountable.is_empty());
}

#[test]
fn subscriptions_create_event_routes() {
    let registry = InMemoryActiveRuntimeRegistry::new();
    registry
        .bind_script(String::from("alpha"), 2, ScriptRetentionPolicy::KeepMounted)
        .unwrap();
    registry
        .set_subscriptions(
            "alpha",
            &[String::from("score.update"), String::from("player.spawn")],
        )
        .unwrap();

    let bindings = registry.get_event_bindings("score.update").unwrap();
    assert_eq!(
        bindings,
        Arc::<[ActiveEventBinding]>::from(vec![ActiveEventBinding {
            script_id: String::from("alpha"),
            worker_id: 2,
            execution_lane: RuntimeExecutionLane::Sync,
        }])
    );
}

#[test]
fn subscription_update_removes_stale_route() {
    let registry = InMemoryActiveRuntimeRegistry::new();
    registry
        .bind_script(String::from("alpha"), 2, ScriptRetentionPolicy::KeepMounted)
        .unwrap();
    registry
        .set_subscriptions("alpha", &[String::from("score.update")])
        .unwrap();

    registry
        .set_subscriptions("alpha", &[String::from("player.spawn")])
        .unwrap();

    assert!(
        registry
            .get_event_bindings("score.update")
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        registry.get_event_bindings("player.spawn").unwrap().len(),
        1
    );
}

#[test]
fn rebinding_script_preserves_hot_routes_until_subscriptions_update() {
    let registry = InMemoryActiveRuntimeRegistry::new();
    registry
        .bind_script(String::from("alpha"), 1, ScriptRetentionPolicy::KeepMounted)
        .unwrap();
    registry
        .set_subscriptions("alpha", &[String::from("score.update")])
        .unwrap();

    registry
        .bind_script(String::from("alpha"), 2, ScriptRetentionPolicy::KeepMounted)
        .unwrap();

    let bindings = registry.get_event_bindings("score.update").unwrap();
    assert_eq!(
        bindings,
        Arc::<[ActiveEventBinding]>::from(vec![ActiveEventBinding {
            script_id: String::from("alpha"),
            worker_id: 2,
            execution_lane: RuntimeExecutionLane::Sync,
        }])
    );
}

#[test]
fn demount_when_idle_triggers_after_execution_without_subscriptions() {
    let registry = InMemoryActiveRuntimeRegistry::new();
    registry
        .bind_script(
            String::from("alpha"),
            2,
            ScriptRetentionPolicy::DemountWhenIdle,
        )
        .unwrap();

    registry.begin_execution("alpha").unwrap();
    let should_demount = registry.end_execution("alpha").unwrap();

    assert!(should_demount);
}

#[test]
fn dependency_ref_prevents_idle_demount() {
    let registry = InMemoryActiveRuntimeRegistry::new();
    registry
        .bind_script(
            String::from("alpha"),
            2,
            ScriptRetentionPolicy::DemountWhenIdle,
        )
        .unwrap();

    registry.retain_dependency("alpha").unwrap();
    registry.begin_execution("alpha").unwrap();
    let should_demount = registry.end_execution("alpha").unwrap();

    assert!(!should_demount);
}

#[test]
fn releasing_last_dependency_ref_allows_idle_demount() {
    let registry = InMemoryActiveRuntimeRegistry::new();
    registry
        .bind_script(
            String::from("alpha"),
            2,
            ScriptRetentionPolicy::DemountWhenIdle,
        )
        .unwrap();

    registry.retain_dependency("alpha").unwrap();
    let should_demount = registry.release_dependency("alpha").unwrap();

    assert!(should_demount);
}

#[test]
fn script_dependency_edge_is_deduplicated() {
    let registry = InMemoryActiveRuntimeRegistry::new();
    registry
        .bind_script(
            String::from("consumer"),
            1,
            ScriptRetentionPolicy::KeepMounted,
        )
        .unwrap();
    registry
        .bind_script(
            String::from("dependency"),
            2,
            ScriptRetentionPolicy::DemountWhenIdle,
        )
        .unwrap();

    registry
        .retain_script_dependency("consumer", "dependency")
        .unwrap();
    registry
        .retain_script_dependency("consumer", "dependency")
        .unwrap();
    let demountable = registry
        .release_script_dependency("consumer", "dependency")
        .unwrap();

    assert_eq!(demountable, vec![(String::from("dependency"), 2)]);
}

#[test]
fn unbinding_dependent_releases_dependency_ref() {
    let registry = InMemoryActiveRuntimeRegistry::new();
    registry
        .bind_script(
            String::from("consumer"),
            1,
            ScriptRetentionPolicy::KeepMounted,
        )
        .unwrap();
    registry
        .bind_script(
            String::from("dependency"),
            2,
            ScriptRetentionPolicy::DemountWhenIdle,
        )
        .unwrap();
    registry
        .retain_script_dependency("consumer", "dependency")
        .unwrap();

    let demountable = registry.unbind_script("consumer").unwrap();

    assert_eq!(demountable, vec![(String::from("dependency"), 2)]);
}
