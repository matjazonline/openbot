//! The run list's statement shapes and its slow-read threshold. The shapes are also what
//! `scripts/tests/run-list-query-plan.py` measures, through `dump_run_list_query_shapes`.
use super::*;

/// One statement shape the run list can issue, and what each of its parameters carries.
struct Shape {
    name: &'static str,
    filter: RunFilter,
    cursor: bool,
}

/// What a `$n` of a shape binds, in placeholder order, with the text it must follow in the
/// statement: the check that the dump's parameter list matches the shipped builder.
fn parameters(shape: &Shape) -> Vec<(&'static str, &'static str)> {
    let agent = shape.filter.agent.is_some();
    let mut parameters = Vec::new();
    if agent {
        parameters.push(("company", "WHERE version.company_id = "));
        parameters.push(("agent", " AND "));
    }
    parameters.push(("company", "FROM workflow_runs WHERE company_id = "));
    if shape.filter.binding.is_some() {
        parameters.push(("binding", " AND binding_id = "));
    }
    if shape.filter.state.is_some() {
        parameters.push(("state", " AND state = "));
    }
    if shape.cursor {
        parameters.push(("cursor_created", " AND (created_at, id) < ("));
        parameters.push(("cursor_id", ", "));
    }
    parameters.push(("limit", " LIMIT "));
    if agent {
        // The merge of the per-version ranges takes the same page size.
        parameters.push(("limit", " LIMIT "));
    }
    parameters
}

fn shapes() -> Vec<Shape> {
    let binding = Some(WorkflowBindingId::new(Uuid::from_u128(2)));
    let agent = Some(Uuid::from_u128(3));
    let (any, top) = (RunLineage::Any, RunLineage::TopLevel);
    let shape = |name, binding, state: Option<&str>, agent, lineage, cursor| Shape {
        name,
        filter: RunFilter {
            binding,
            state: state.map(str::to_owned),
            agent,
            lineage,
        },
        cursor,
    };
    vec![
        shape("company", None, None, None, any, false),
        shape("binding", binding, None, None, any, false),
        shape("state", None, Some("failed"), None, any, false),
        shape("agent", None, None, agent, any, false),
        shape("agent_state", None, Some("failed"), agent, any, false),
        shape("agent_cursor", None, None, agent, any, true),
        shape("top_level", None, None, None, top, false),
        shape("agent_top_level", None, None, agent, top, false),
    ]
}

fn statement(shape: &Shape) -> String {
    let cursor = shape
        .cursor
        .then(|| (DateTime::<Utc>::UNIX_EPOCH, Uuid::from_u128(4)));
    run_list_query(
        CompanyId::new(Uuid::from_u128(1)),
        &shape.filter,
        cursor,
        51,
    )
    .sql()
    .to_owned()
}

#[test]
fn run_list_query_binds_each_present_filter_once_in_placeholder_order() {
    for shape in shapes() {
        let sql = statement(&shape);
        let parameters = parameters(&shape);
        for (index, (role, after)) in parameters.iter().enumerate() {
            let placeholder = format!("{after}${}", index + 1);
            assert!(
                sql.contains(&placeholder),
                "{}: {role} {placeholder} in {sql}",
                shape.name
            );
        }
        assert!(
            !sql.contains(&format!("${}", parameters.len() + 1)),
            "{}: more placeholders than parameters in {sql}",
            shape.name
        );
        assert_eq!(
            sql.contains("agent_ids"),
            shape.filter.agent.is_some(),
            "{}: {sql}",
            shape.name
        );
        assert_eq!(
            sql.contains("binding_id ="),
            shape.filter.binding.is_some(),
            "{}: {sql}",
            shape.name
        );
        // Filtered in SQL, so pages stay full and the cursor never passes rows nobody saw; inside
        // each version's range when an agent is present.
        assert_eq!(
            sql.matches(" AND parent_run_id IS NULL").count(),
            usize::from(shape.filter.lineage == RunLineage::TopLevel),
            "{}: {sql}",
            shape.name
        );
        assert!(sql.ends_with(&format!(
            "ORDER BY created_at DESC, id DESC LIMIT ${}",
            parameters.len()
        )));
    }
}

#[test]
fn the_slow_read_log_names_a_run_state_and_never_the_callers_text() {
    for state in [
        "queued",
        "running",
        "waiting",
        "succeeded",
        "failed",
        "cancelled",
    ] {
        assert_eq!(logged_state(Some(state)), state);
    }
    assert_eq!(logged_state(None), "none");
    assert_eq!(logged_state(Some("failed\nforged line")), "other");
    assert_eq!(logged_state(Some("FAILED")), "other");
}

#[test]
fn the_slow_read_log_names_the_lineage() {
    assert_eq!(logged_lineage(RunLineage::Any), "any");
    assert_eq!(logged_lineage(RunLineage::TopLevel), "top_level");
}

#[test]
fn run_list_is_slow_only_past_the_threshold() {
    let threshold = RUN_LIST_QUERY_WARN_THRESHOLD;
    assert!(!is_slow(Duration::ZERO, threshold));
    assert!(!is_slow(threshold, threshold));
    assert!(is_slow(threshold + Duration::from_millis(1), threshold));
}

/// Writes every shipped run-list shape, with what each `$n` binds, as JSON to the file named by
/// `RUN_LIST_QUERY_SHAPES`, for `scripts/tests/run-list-query-plan.py`:
///
/// ```sh
/// RUN_LIST_QUERY_SHAPES=/tmp/shapes.json \
///   cargo test --locked --offline --lib dump_run_list_query_shapes -- --ignored
/// ```
#[test]
#[ignore = "writes the query-plan script's input; run on demand"]
fn dump_run_list_query_shapes() {
    let path = std::env::var("RUN_LIST_QUERY_SHAPES")
        .expect("RUN_LIST_QUERY_SHAPES names the output file");
    let shapes: Vec<Value> = shapes()
        .iter()
        .map(|shape| {
            serde_json::json!({
                "name": shape.name,
                "sql": statement(shape),
                "parameters": parameters(shape).iter().map(|(role, _)| *role).collect::<Vec<_>>(),
            })
        })
        .collect();
    std::fs::write(path, serde_json::to_string_pretty(&shapes).unwrap()).unwrap();
}
