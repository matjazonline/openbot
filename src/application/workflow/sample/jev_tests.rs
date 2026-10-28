use super::*;
use crate::application::workflow::jev::assets::tests as images;
#[test]
fn jev_box_sample_uses_only_scripted_answers_and_inert_metadata() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../../../samples/jev-image.json")).unwrap();
    let bundle = build(&fixture["source"], DependencySnapshots::default(), vec![]);
    let frozen = crate::application::workflow::publication::store_bundle(&bundle).unwrap();
    let report = execute(
        bundle,
        serde_json::from_value(fixture["sample"].clone()).unwrap(),
    )
    .unwrap();
    complete(&report);
    assert_eq!(report.usage.model_calls, 1);
    assert_eq!(report.usage.scripted_tokens, 220);
    assert!(report.effects.is_empty());
    assert_eq!(report.output.as_ref().unwrap(), &images::output());
    let serialized = serde_json::to_string(&report).unwrap();
    assert!(!serialized.contains("data_base64"));
    assert!(!String::from_utf8_lossy(&frozen).contains("data_base64"));
}

#[test]
fn workflow_sample_jev_mixed_thresholds_automatic_and_human_paths() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../../../samples/jev-triage.json")).unwrap();
    let bundle = build(&fixture["source"], DependencySnapshots::default(), vec![]);
    for (name, human) in [("sample", false), ("uncertain_sample", true)] {
        let report = execute(
            bundle.clone(),
            serde_json::from_value(fixture[name].clone()).unwrap(),
        )
        .unwrap();
        complete(&report);
        assert_eq!(report.usage.model_calls, 1);
        assert_eq!(report.usage.scripted_tokens, 132);
        assert!(report.effects.is_empty());
        assert_eq!(
            report
                .steps
                .iter()
                .any(|step| step.kind.as_str() == "decision.human"),
            human
        );
        assert_eq!(report.steps.len(), 3);
    }
    let mut over_budget = fixture["sample"].clone();
    over_budget["scripts"][0]["responses"][0]["output"]["usage"]["input_tokens"] = json!(1_000_001);
    let report = execute(bundle, serde_json::from_value(over_budget).unwrap()).unwrap();
    assert_eq!(report.status, SampleStatus::Stopped);
}

#[test]
fn jev_box_sample_rejects_tools_tokens_and_invalid_box() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../../../samples/jev-image.json")).unwrap();
    let bundle = build(&fixture["source"], DependencySnapshots::default(), vec![]);
    for edit in 0..3 {
        let mut request = fixture["sample"].clone();
        match edit {
            0 => {
                request["scripts"][0]["responses"][0]["tools"] =
                    json!([{"name":"anything","input":{},"output":{}}])
            }
            1 => request["scripts"][0]["responses"][0]["tokens"] = json!(1_000_001),
            _ => {
                request["scripts"][0]["responses"][0]["output"]["answers"]["mug"]["box"]["left"] =
                    json!(0.9)
            }
        }
        let report = execute(bundle.clone(), serde_json::from_value(request).unwrap()).unwrap();
        assert_eq!(report.status, SampleStatus::Stopped);
        assert!(report.effects.is_empty());
    }
}
