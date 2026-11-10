use super::*;
use crate::application::agent::ActionEffect;
use crate::application::workflow::{
    compiler::{Schema, SourceSpan},
    memory_tools::{MemoryRecallRef, MemoryToolScope, save_sources},
    provisioning::is_reserved_tool_name,
};
use crate::domain::workflow::ResourceName;
use crate::entities::access_level::AccessLevel;
use uuid::Uuid;

fn span() -> SourceSpan {
    SourceSpan {
        start: 0,
        end: 0,
        line: 1,
        column: 1,
    }
}

fn valid(schema: &Value, value: &Value) -> bool {
    Schema::compile(schema.clone(), "/input", span())
        .unwrap()
        .validate(value, "/input", span())
        .is_ok()
}

/// No namespace, tenant, user or level is ever a model input (E5), and every bound holds.
#[test]
fn memory_scope_tool_inputs_carry_no_authority_and_enforce_their_bounds() {
    for tool in MemoryTool::ALL {
        let input = memory_tool_schemas(tool).input;
        assert_eq!(input["additionalProperties"], false, "{}", tool.name());
        for field in [
            "namespace",
            "tenant",
            "user",
            "user_id",
            "level",
            "access_level",
        ] {
            assert!(
                input["properties"].get(field).is_none(),
                "{}: {field}",
                tool.name()
            );
        }
    }
    let search = memory_tool_schemas(MemoryTool::Search).input;
    assert!(valid(
        &search,
        &json!({"query":"x","scopes":["company","user"],"limit":10})
    ));
    for bad in [
        json!({"query":""}),
        json!({"query":"x".repeat(1025)}),
        json!({"query":"x","limit":11}),
        json!({"query":"x","scopes":[]}),
        json!({"query":"x","scopes":["agent","agent"]}),
        json!({"query":"x","scopes":["tenant"]}),
        json!({"query":"x","level":9}),
    ] {
        assert!(!valid(&search, &bad), "{bad}");
    }
    let timeline = memory_tool_schemas(MemoryTool::Timeline).input;
    let id = Uuid::new_v4();
    assert!(valid(
        &timeline,
        &json!({"scope":"agent","id":id,"before":5,"after":0})
    ));
    assert!(!valid(
        &timeline,
        &json!({"scope":"agent","id":id,"after":6})
    ));
    let details = memory_tool_schemas(MemoryTool::Details).input;
    let item = json!({"scope":"company","id":id});
    assert!(valid(&details, &json!({"items":vec![item.clone();4]})));
    assert!(!valid(&details, &json!({"items":vec![item;5]})));
    assert!(!valid(&details, &json!({"items":[]})));
    let save = memory_tool_schemas(MemoryTool::Save).input;
    assert!(valid(&save, &json!({"facts":vec!["x";8],"scope":"user"})));
    for bad in [
        json!({"facts":vec!["x";9]}),
        json!({"facts":["x".repeat(4097)]}),
        json!({"facts":["x"],"scope":"company"}),
        json!({"facts":[]}),
    ] {
        assert!(!valid(&save, &bad), "{bad}");
    }
}

/// A snapshot is a memory tool only when it equals the built-in exactly; any widening or a
/// connection makes it something else, and the names are reserved from other registrations.
#[test]
fn memory_scope_builtin_snapshots_are_reserved_and_exactly_trusted() {
    let company = CompanyId::new(Uuid::new_v4());
    for tool in MemoryTool::ALL {
        assert!(is_reserved_tool_name(tool.name()));
        assert_eq!(MemoryTool::from_builtin(tool.builtin()), Some(tool));
        let snapshot = builtin_memory_snapshot(company, tool).unwrap();
        assert_eq!(snapshot.kind(), ToolKind::Memory);
        assert_eq!(trusted_memory_tool(&snapshot).unwrap(), Some(tool));
        let mut external = snapshot.clone();
        external.contract.connection = Some(ResourceName::parse("external").unwrap());
        assert_eq!(trusted_memory_tool(&external).unwrap(), None);
        let mut widened = snapshot.clone();
        widened.contract.input_schema = json!({"type":"object"});
        assert_eq!(trusted_memory_tool(&widened).unwrap(), None);
        let mut output = snapshot.clone();
        output.contract.output_schema = json!({});
        assert_eq!(trusted_memory_tool(&output).unwrap(), None);
        let mut effect = snapshot.clone();
        effect.policy.effect = match tool.effect() {
            ActionEffect::Read => ActionEffect::Write,
            _ => ActionEffect::Read,
        };
        assert_eq!(trusted_memory_tool(&effect).unwrap(), None);
        let mut revision = snapshot;
        revision.policy.policy_revision = 2;
        assert_eq!(trusted_memory_tool(&revision).unwrap(), None);
    }
    assert_eq!(MemoryTool::Save.effect(), ActionEffect::Write);
    assert!(!is_builtin_memory_tool("search_knowledge"));
}

/// A tool save cites the distinct observations its execution recalled, highest level first,
/// then by id, at most 64 (E5).
#[test]
fn memory_access_level_save_sources_are_distinct_level_ordered_and_bounded() {
    let level = |value: i16| AccessLevel::try_from(value).unwrap();
    let recall = |scope, id: u128, value: i16| MemoryRecallRef {
        scope,
        id: Uuid::from_u128(id),
        revision: 1,
        access_level: level(value),
        content_sha256: None,
        sources_sha256: None,
    };
    let sources = save_sources([
        recall(MemoryToolScope::Company, 2, 1),
        recall(MemoryToolScope::Agent, 3, 4),
        recall(MemoryToolScope::Company, 2, 6),
        recall(MemoryToolScope::User, 1, 4),
        recall(MemoryToolScope::Agent, 3, 2),
    ]);
    let listed: Vec<_> = sources
        .iter()
        .map(|source| (source.reference.as_str(), source.access_level))
        .collect();
    assert_eq!(
        listed,
        vec![
            ("memory:company:00000000-0000-0000-0000-000000000002", 6),
            ("memory:user:00000000-0000-0000-0000-000000000001", 4),
            ("memory:agent:00000000-0000-0000-0000-000000000003", 4),
        ]
    );
    let many =
        save_sources((0..100).map(|id| recall(MemoryToolScope::Agent, id, (id % 10) as i16)));
    assert_eq!(many.len(), 64);
    assert!(
        many.windows(2)
            .all(|pair| pair[0].access_level >= pair[1].access_level)
    );
    assert!(save_sources([]).is_empty());
}

/// E2 with U4: the user scope is the actor's own manual root in an unattached session they
/// created, inherited by children run as the same actor. Every other root is refused, whoever
/// authored the workflow (authorship is not a fact under U4).
#[test]
fn memory_scope_user_scope_eligibility_matrix() {
    use crate::application::workflow::memory_tools::{UserScopeRoot, user_scope_owner};
    let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
    let own = UserScopeRoot {
        actor: a,
        manual_start: true,
        session_creator: a,
        session_attached: false,
    };
    // A manual root of A's (whoever published the workflow) and a child of it run as A.
    assert_eq!(user_scope_owner(a, Some(&own)), Some(a));
    // A child run as someone else never inherits A's scope.
    assert_eq!(user_scope_owner(b, Some(&own)), None);
    let refused = [
        // A schedule (run-as A) or any non-manual root.
        UserScopeRoot {
            manual_start: false,
            ..own
        },
        // A message-triggered root, and so every child of a thread run.
        UserScopeRoot {
            manual_start: false,
            session_attached: true,
            ..own
        },
        // A manual run in a channel or thread: an attached session.
        UserScopeRoot {
            session_attached: true,
            ..own
        },
        // A session someone else created.
        UserScopeRoot {
            session_creator: b,
            ..own
        },
    ];
    for root in refused {
        assert_eq!(user_scope_owner(a, Some(&root)), None, "{root:?}");
    }
    assert_eq!(user_scope_owner(a, None), None, "no root, no scope");
}
