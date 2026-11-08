use super::*;
use crate::application::workflow::agent_history::thread_history_input_proven;

#[test]
fn agent_history_identity_public_lineage_is_closed_and_does_not_include_future_reviews() {
    for fixture in [Fixture::AutonomousResponse, Fixture::ReviewedSupport] {
        let bundle = bundle(fixture);
        assert!(thread_history_input_proven(&bundle, &step("draft"), &[step("context")]).unwrap());
    }
    let bundle = bundle(Fixture::ReviewedSupport);
    assert!(
        !thread_history_input_proven(
            &bundle,
            &step("reply"),
            &[step("context"), step("draft"), step("review")]
        )
        .unwrap()
    );
    assert!(
        !thread_history_input_proven(&bundle, &step("draft"), &[step("context"), step("review")])
            .unwrap(),
        "executed private control cannot be hidden behind a later public-looking value"
    );
}

fn changed_bundle(change: impl FnOnce(&mut Value)) -> PublishedBundle {
    let mut source: Value = serde_yaml::from_str(Fixture::ReviewedSupport.source()).unwrap();
    change(&mut source);
    build(Fixture::ReviewedSupport, &source.to_string()).unwrap()
}

#[test]
fn agent_history_identity_private_params_and_transitive_inputs_never_use_thread_stream() {
    let direct = changed_bundle(|source| {
        source["steps"]["draft"]["with"]["context"] = json!({"ref":"/params/reviewer"});
    });
    assert!(!thread_history_input_proven(&direct, &step("draft"), &[]).unwrap());
    let indirect = changed_bundle(|source| {
        source["steps"]["context"] = json!({"type":"data.map","with":{
            "value":{"ref":"/params/reviewer"},"output_schema":{"literal":true}},
            "routes":{"success":"draft"}});
    });
    assert!(!thread_history_input_proven(&indirect, &step("draft"), &[]).unwrap());
}

#[test]
fn agent_history_identity_provenance_rejects_unknown_and_over_budget_execution_sets() {
    let bundle = bundle(Fixture::AutonomousResponse);
    assert!(thread_history_input_proven(&bundle, &step("missing"), &[]).is_err());
    let too_many = vec![step("context"); bundle.compiled().graph().definition().steps.len() + 1];
    assert!(thread_history_input_proven(&bundle, &step("draft"), &too_many).is_err());
}
