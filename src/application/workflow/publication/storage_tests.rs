use super::*;
use crate::adapters::workflow_source::{WorkflowSourceDecoder, decode};
use crate::application::workflow::fixtures::{ALL, Fixture};
use uuid::Uuid;

fn fixture_bundle(fixture: Fixture) -> Arc<PublishedBundle> {
    let company = CompanyId::new(Uuid::from_u128(91));
    Arc::new(
        freeze(
            decode(fixture.source()).unwrap(),
            company,
            fixture.version_id(),
            fixture.snapshots(company),
            fixture.child().map(fixture_bundle).into_iter().collect(),
        )
        .unwrap(),
    )
}

#[test]
fn workflow_storage_restores_all_frozen_fixtures_and_child_identity() {
    for fixture in ALL {
        let original = fixture_bundle(fixture);
        let bytes = store_bundle(&original).unwrap();
        let restored = restore_bundle(&bytes, &WorkflowSourceDecoder).unwrap();
        assert_eq!(restored.content_hash(), original.content_hash());
        assert_eq!(restored.compiled().source(), original.compiled().source());
        assert_eq!(
            restored.compiled().representation(),
            original.compiled().representation()
        );
        assert_eq!(restored.children().len(), original.children().len());
        assert_eq!(store_bundle(&restored).unwrap(), bytes);
        assert_eq!(
            restored.compiled().graph().definition().limits.root_budget,
            original.compiled().graph().definition().limits.root_budget
        );
    }
}

#[test]
fn workflow_root_budget_storage_roundtrip_and_old_semantics_refusal() {
    let fixture = Fixture::AutonomousResponse;
    let company = CompanyId::new(Uuid::from_u128(91));
    let source = fixture.source().replace(
        "limits: {",
        "limits: {root_budget: {activations: 9, model_calls: 4, repetitions: 2}, ",
    );
    let original = freeze(
        decode(&source).unwrap(),
        company,
        fixture.version_id(),
        fixture.snapshots(company),
        vec![],
    )
    .unwrap();
    let bytes = store_bundle(&original).unwrap();
    let restored = restore_bundle(&bytes, &WorkflowSourceDecoder).unwrap();
    assert_eq!(
        restored.compiled().graph().definition().limits.root_budget,
        crate::domain::workflow::RootBudgetLimits::new(9, 4, 2).unwrap()
    );
    assert_eq!(store_bundle(&restored).unwrap(), bytes);
    let mut stored: Value = serde_json::from_slice(&bytes).unwrap();
    stored["compiler_revision"] = json!(1);
    assert!(
        restore_bundle(
            &serde_json::to_vec(&stored).unwrap(),
            &WorkflowSourceDecoder
        )
        .is_err()
    );
    stored["compiler_revision"] = json!(compiler::SEMANTIC_REVISION);
    stored["versions"][0]["compiled"]["root_budget"]["model_calls"] = json!(5);
    assert!(
        restore_bundle(
            &serde_json::to_vec(&stored).unwrap(),
            &WorkflowSourceDecoder
        )
        .is_err()
    );
}

#[test]
fn workflow_storage_rejects_corruption_and_compiler_drift() {
    let original = fixture_bundle(ALL[0]);
    let pristine: Value = serde_json::from_slice(&store_bundle(&original).unwrap()).unwrap();
    for mutation in 0..8 {
        let mut stored = pristine.clone();
        match mutation {
            0 => stored["format"] = json!("future"),
            1 => stored["versions"][0]["hash"] = json!("forged"),
            2 => stored["versions"][0]["compiled"]["future_default"] = json!(true),
            3 => stored["versions"][0]["manifest"]["compiled_hash"] = json!("forged"),
            4 => stored["versions"][0]["snapshots"]["agents"][0]["key"] = json!("bad key"),
            5 => stored["versions"][0]["children"] = json!([Uuid::new_v4()]),
            6 => {
                stored["versions"][0]["children"] =
                    json!([original.compiled().graph().definition().version_id])
            }
            _ => {
                let duplicate = stored["versions"][0].clone();
                stored["versions"].as_array_mut().unwrap().push(duplicate);
            }
        }
        assert!(
            restore_bundle(
                &serde_json::to_vec(&stored).unwrap(),
                &WorkflowSourceDecoder
            )
            .is_err(),
            "mutation {mutation}"
        );
    }
    assert!(restore_bundle(&vec![b' '; MAX_BUNDLE_BYTES + 1], &WorkflowSourceDecoder).is_err());
    assert!(restore_bundle(b"{", &WorkflowSourceDecoder).is_err());
}

#[test]
fn workflow_storage_deserialization_preserves_name_invariants() {
    for invalid in ["", "$end", "invalid name", "double..dot"] {
        assert!(
            serde_json::from_value::<crate::domain::workflow::TypeName>(json!(invalid)).is_err()
        );
    }
}

#[test]
fn workflow_storage_rejects_incompatible_compiler_before_rebuilding() {
    struct MustNotDecode;
    impl SourceDecoder for MustNotDecode {
        fn decode(&self, _: &str) -> Result<DecodedSource, Diagnostic> {
            panic!("incompatible compiler must be rejected before decoding");
        }
    }
    let original = fixture_bundle(ALL[0]);
    let mut stored: Value = serde_json::from_slice(&store_bundle(&original).unwrap()).unwrap();
    stored["compiler_revision"] = json!(compiler::SEMANTIC_REVISION + 1);
    assert!(restore_bundle(&serde_json::to_vec(&stored).unwrap(), &MustNotDecode).is_err());
    stored.as_object_mut().unwrap().remove("compiler_revision");
    assert!(restore_bundle(&serde_json::to_vec(&stored).unwrap(), &MustNotDecode).is_err());
}
