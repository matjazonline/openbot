use super::*;
use crate::application::memory::MemoryHit;

/// The host's own parse of model arguments, behind the schema: unknown keys, a company save,
/// duplicate scopes and blank text refuse; omitted fields take their documented defaults.
#[test]
fn memory_scope_tool_arguments_parse_strictly_with_documented_defaults() {
    let search = SearchInput::parse(&json!({"query":"x"})).unwrap();
    assert_eq!((search.scopes, search.limit), (None, 5));
    for bad in [
        json!({"query":"   "}),
        json!({"query":"x","scopes":["agent","agent"]}),
        json!({"query":"x","scopes":[]}),
        json!({"query":"x","limit":0}),
        json!({"query":"x","level":3}),
    ] {
        assert!(SearchInput::parse(&bad).is_none(), "{bad}");
    }
    let id = Uuid::new_v4();
    let timeline = TimelineInput::parse(&json!({"scope":"user","id":id})).unwrap();
    assert_eq!(
        (timeline.scope, timeline.before, timeline.after),
        (MemoryToolScope::User, 5, 5)
    );
    assert!(TimelineInput::parse(&json!({"scope":"user","id":"not-a-uuid"})).is_none());
    let details = DetailsInput::parse(
        &json!({"items":[{"scope":"agent","id":id},{"scope":"agent","id":id}]}),
    )
    .unwrap();
    assert_eq!(details.items, vec![(MemoryToolScope::Agent, id)]);
    let save = SaveInput::parse(&json!({"facts":["Prefers mornings."]})).unwrap();
    assert_eq!(save.scope, MemoryToolScope::Agent, "default agent");
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
        .map(|level| (MemoryToolScope::Company, hit(level)))
        .collect();
    let (result, refs) = bounded_hits(&hits).unwrap();
    let shown = result["items"].as_array().unwrap();
    assert!(!shown.is_empty() && shown.len() < hits.len());
    assert!(result.to_string().len() <= result_bound());
    assert_eq!(refs.len(), shown.len());
    for (shown, recorded) in shown.iter().zip(&refs) {
        assert_eq!(shown["id"], json!(recorded.id));
        assert_eq!(shown["access_level"], json!(recorded.access_level));
    }
    let invalid = [(MemoryToolScope::Agent, hit(10))];
    assert!(
        bounded_hits(&invalid).is_err(),
        "an out-of-range level is a fault"
    );
}
