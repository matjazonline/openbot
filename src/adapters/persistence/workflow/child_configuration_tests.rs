//! A child's own configuration (07-D1 §D1.2, plan 07 §Child calls): the step's frozen static
//! parameters, and the parent's bound resources for exactly the child's declared slots.
use super::*;
use crate::application::workflow::children::WorkflowChildren;
use crate::application::workflow::waits::*;

/// A leaf that requires a short `reviewer` parameter and one MCP slot, `service`.
fn leaf() -> Value {
    let mut source: Value =
        serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
    source["parameter_schema"] = json!({"type":"object","properties":{"reviewer":{"type":"string","maxLength":8}},"required":["reviewer"],"additionalProperties":false});
    source["resources"] = json!([{"slot":"service","kind":"mcp"}]);
    source
}

/// A root that holds two MCP slots and passes its own `reviewer` parameter to the child; a
/// refused child configuration takes the step's error route to `recover`.
fn parent(child: WorkflowId, version: VersionId) -> Value {
    let mut source = child_support::call_source(child, version);
    source["parameter_schema"] = json!({"type":"object","properties":{"reviewer":{"type":"string"}},"required":["reviewer"],"additionalProperties":false});
    source["resources"] = json!([{"slot":"service","kind":"mcp"},{"slot":"archive","kind":"mcp"}]);
    source["steps"]["start"]["with"]["params"] =
        json!({"object":{"reviewer":{"ref":"/params/reviewer"}}});
    source["steps"]["start"]["routes"]["error"] = json!("recover");
    source["steps"]["recover"] = json!({"type":"data.map","with":{"value":{"literal":"recovered"},"output_schema":{"literal":true}},"routes":{"success":"$end"}});
    source
}

async fn configuration(f: &AdmissionFixture, run: RunId) -> (Value, Value, Uuid) {
    sqlx::query_as("SELECT params, resources, binding_id FROM workflow_runs WHERE id=$1")
        .bind(run.as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn workflow_child_gets_its_frozen_params_and_only_its_own_resource_slots() {
    let (f, root) = AdmissionFixture::with_bound_child(
        leaf(),
        parent,
        json!({"reviewer":"r1"}),
        &["service", "archive"],
    )
    .await;
    let child = f.spawn(root).await;
    let (root_params, root_resources, root_binding) = configuration(&f, root.run).await;
    let (params, resources, binding) = configuration(&f, child.run).await;
    assert_eq!(root_params, json!({"reviewer":"r1"}));
    assert_eq!(root_resources.as_object().unwrap().len(), 2);
    // Exactly the step's parameters, and the parent's resource for the child's one slot.
    assert_eq!(params, json!({"reviewer":"r1"}));
    assert_eq!(resources, json!({"service": root_resources["service"]}));
    // The binding identity is provenance only; the child's configuration is its own row.
    assert_eq!(binding, root_binding);
    // The child runs under that configuration to completion, and the parent consumes it.
    f.persistence()
        .advance_pure(child, budget(4))
        .await
        .unwrap();
    assert!(matches!(
        f.persistence().resume_wait(root).await.unwrap(),
        WaitProgress::Completed(_)
    ));
}

#[tokio::test]
async fn workflow_child_parameters_the_child_refuses_fail_activation_without_a_child() {
    // The parent's own parameter is valid for the parent and too long for the child. The
    // step's `params` contract embeds the child's parameter schema, so activation refuses the
    // frozen inputs: like any step's invalid input, the run fails unrouted and no child exists.
    // (A refusal after activation takes the error route: the child-limits case.)
    let (f, root) = AdmissionFixture::with_bound_child(
        leaf(),
        parent,
        json!({"reviewer":"far-too-long"}),
        &["service", "archive"],
    )
    .await;
    let p = f.persistence();
    assert!(p.park_child(root).await.unwrap().is_none());
    let observed = || async {
        sqlx::query_as::<_, (String, i64, i64, i64)>(
            "SELECT (SELECT state FROM workflow_runs WHERE id=$1), \
             (SELECT count(*) FROM workflow_runs WHERE parent_run_id=$1), \
             (SELECT count(*) FROM workflow_run_events WHERE run_id=$1 AND event_kind='workflow.invalid_input'), \
             (SELECT count(*) FROM workflow_executions WHERE run_id=$1 AND step_id='recover')",
        )
        .bind(root.run.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap()
    };
    assert_eq!(observed().await, ("failed".into(), 0, 1, 0));
    // A replay of the park is settled and changes nothing.
    assert!(p.park_child(root).await.unwrap().is_none());
    assert_eq!(observed().await, ("failed".into(), 0, 1, 0));
}
