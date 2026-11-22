use super::*;
use crate::application::memory::MemoryHit;

#[test]
fn memory_scope_tools_query_proofs_bind_empty_results_and_exact_namespace_references() {
    let namespace = NamespaceExternalId(Uuid::new_v4());
    let mut observation = MemoryReadObservation {
        result: json!({"items":[]}),
        namespaces: vec![namespace],
        exposed: AccessLevel::PUBLIC,
        read_ceiling: AccessLevel::L5,
        items: Vec::new(),
    };
    observation.validate_items().unwrap();
    observation.namespaces.clear();
    assert!(observation.validate_items().is_err());
    observation.namespaces = (0..crate::application::workflow::scopes::MAX_RUN_READ_NAMESPACES)
        .map(|_| NamespaceExternalId(Uuid::new_v4()))
        .collect();
    observation.validate_items().unwrap();
    observation.namespaces.push(namespace);
    assert!(observation.validate_items().is_err());
    observation.namespaces = vec![namespace];
    let id = Uuid::new_v4();
    observation.exposed = AccessLevel::L2;
    observation.result = json!({"items":[{"scope":"unit:root","namespace":namespace,"id":id,"revision":1,
        "kind":"fact","title":"Fact","snippet":"Text","occurred_at":"2026-11-01T00:00:00Z","access_level":2}]});
    observation.items.push(MemoryRecallRef {
        namespace,
        id,
        revision: 1,
        access_level: AccessLevel::L2,
        content_sha256: None,
        sources_sha256: None,
    });
    observation.validate_items().unwrap();
    observation.items[0].namespace = NamespaceExternalId(Uuid::new_v4());
    assert!(observation.validate_items().is_err());
    observation.items[0].namespace = namespace;
    observation.result["items"] = json!([]);
    assert!(observation.validate_items().is_err());
}

/// The host's own parse of model arguments, behind the schema: unknown keys, a company save,
/// duplicate scopes and blank text refuse; omitted fields take their documented defaults.
#[test]
fn memory_scope_tool_arguments_parse_strictly_with_documented_defaults() {
    let search = SearchInput::parse(&json!({"query":"x"})).unwrap();
    assert_eq!((search.scopes, search.limit), (None, 5));
    for bad in [
        json!({"query":"   "}),
        json!({"query":"x","scopes":["personal","personal"]}),
        json!({"query":"x","scopes":[]}),
        json!({"query":"x","limit":0}),
        json!({"query":"x","level":3}),
    ] {
        assert!(SearchInput::parse(&bad).is_none(), "{bad}");
    }
    let id = Uuid::new_v4();
    let timeline = TimelineInput::parse(&json!({"scopes":["personal"],"id":id})).unwrap();
    assert_eq!(
        (timeline.scopes, timeline.before, timeline.after),
        (Some(vec![MemoryToolScope::Personal]), 5, 5)
    );
    assert!(TimelineInput::parse(&json!({"scopes":["personal"],"id":"not-a-uuid"})).is_none());
    let namespace = NamespaceExternalId(Uuid::new_v4());
    let details = DetailsInput::parse(
        &json!({"items":[{"namespace":namespace,"id":id},{"namespace":namespace,"id":id}]}),
    )
    .unwrap();
    assert_eq!(details.items, vec![(namespace, id)]);
    let save = SaveInput::parse(&json!({"facts":["Prefers mornings."]})).unwrap();
    assert_eq!(
        save.target,
        MemorySaveTarget::Default,
        "default local unit or personal fallback"
    );
    assert!(SaveInput::parse(&json!({"facts":["x"],"scope":"company"})).is_none());
    assert!(SaveInput::parse(&json!({"facts":["  "]})).is_none());
}

/// Trailing hits are dropped to fit the result bound, and the recorded references are exactly
/// the hits the model is shown.
#[test]
fn memory_access_level_bounded_hits_record_exactly_what_is_shown() {
    let hit = |level: u8| MemoryHit {
        observation_id: Uuid::new_v4(),
        kind: "fact".into(),
        title: "t".into(),
        snippet: "s".repeat(result_bound() / 3),
        occurred_at: chrono::Utc::now(),
        access_level: level,
        revision: 1,
    };
    let hits: Vec<_> = (0..5)
        .map(|level| LabeledHit {
            namespace: NamespaceExternalId(Uuid::new_v4()),
            label: "unit:root".into(),
            archived: false,
            hit: hit(level),
        })
        .collect();
    let (result, refs) = bounded_hits(&hits).unwrap();
    let shown = result["items"].as_array().unwrap();
    assert!(!shown.is_empty() && shown.len() < hits.len());
    assert!(result.to_string().len() <= result_bound());
    assert_eq!(refs.len(), shown.len());
    for (shown, recorded) in shown.iter().zip(&refs) {
        assert_eq!(shown["namespace"], json!(recorded.namespace));
        assert_eq!(shown["id"], json!(recorded.id));
        assert_eq!(shown["access_level"], json!(recorded.access_level));
    }
    let invalid = [LabeledHit {
        namespace: NamespaceExternalId(Uuid::new_v4()),
        label: "personal".into(),
        archived: false,
        hit: hit(10),
    }];
    assert!(
        bounded_hits(&invalid).is_err(),
        "an out-of-range level is a fault"
    );
}
