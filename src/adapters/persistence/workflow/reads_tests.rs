//! The run list's and the run board's statement shapes, and the list's slow-read threshold. The
//! shapes are also what `scripts/tests/run-list-query-plan.py` measures, through
//! `dump_run_list_query_shapes`.
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
    if shape.filter.workflow.is_some() {
        parameters.push(("workflow", " AND workflow_id = "));
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
    let workflow = Some(WorkflowId::new(Uuid::from_u128(6)));
    let (any, top) = (RunLineage::Any, RunLineage::TopLevel);
    let shape = |name, binding, state: Option<&str>, agent, lineage, cursor| Shape {
        name,
        filter: RunFilter {
            binding,
            workflow: None,
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
        Shape {
            name: "workflow",
            filter: RunFilter {
                workflow,
                ..RunFilter::default()
            },
            cursor: false,
        },
        Shape {
            name: "workflow_state",
            filter: RunFilter {
                workflow,
                state: Some("failed".into()),
                ..RunFilter::default()
            },
            cursor: false,
        },
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

/// One board statement: the counts or the cards of a scope.
struct BoardShape {
    name: &'static str,
    cards: bool,
    scope: RunBoardScope,
}

fn board_shapes() -> Vec<BoardShape> {
    let binding = RunBoardScope {
        binding: Some(WorkflowBindingId::new(Uuid::from_u128(2))),
        ..RunBoardScope::default()
    };
    let workflow = RunBoardScope {
        workflow: Some(WorkflowId::new(Uuid::from_u128(6))),
        ..RunBoardScope::default()
    };
    let agent = RunBoardScope {
        agent: Some(Uuid::from_u128(3)),
        ..RunBoardScope::default()
    };
    let shape = |name, cards, scope| BoardShape { name, cards, scope };
    vec![
        shape("board_counts", false, RunBoardScope::default()),
        shape("board_cards", true, RunBoardScope::default()),
        shape("board_counts_binding", false, binding),
        shape("board_cards_binding", true, binding),
        shape("board_counts_workflow", false, workflow),
        shape("board_cards_workflow", true, workflow),
        shape("board_counts_agent", false, agent),
        shape("board_cards_agent", true, agent),
    ]
}

/// The `$n` a board scope binds on the materialized working set, in placeholder order, each with
/// the text it follows.
fn board_scope_parameters(
    scope: &RunBoardScope,
    parameters: &mut Vec<(&'static str, &'static str)>,
) {
    if scope.binding.is_some() {
        parameters.push(("binding", "binding_id = "));
    }
    if scope.workflow.is_some() {
        parameters.push(("workflow", "workflow_id = "));
    }
    if scope.agent.is_some() {
        parameters.push(("company", "WHERE version.company_id = "));
        parameters.push(("agent", " AND "));
    }
}

/// The counts bind the company for their open part, then the company and the cutoff for their
/// finished part, then the scope. The cards bind, per state, the company, the visible channels and
/// the cutoff for a finished state; scoped, those are the materialized columns, and each column's
/// scope follows them all.
fn board_parameters(shape: &BoardShape) -> Vec<(&'static str, &'static str)> {
    let mut parameters = vec![("company", "FROM workflow_runs WHERE company_id = ")];
    if !shape.cards {
        parameters.push(("company", "FROM workflow_runs WHERE company_id = "));
        parameters.push(("cutoff", " AND finished_at >= "));
        board_scope_parameters(&shape.scope, &mut parameters);
        return parameters;
    }
    for (index, state) in RunStateName::ALL.into_iter().enumerate() {
        if index > 0 {
            parameters.push(("company", "FROM workflow_runs WHERE company_id = "));
        }
        parameters.push(("channels", " AND (channel_id = ANY("));
        parameters.push(("company_visible", ") OR (channel_id IS NULL AND "));
        if !state.is_open() {
            parameters.push(("cutoff", " AND finished_at >= "));
        }
    }
    for _ in RunStateName::ALL {
        board_scope_parameters(&shape.scope, &mut parameters);
    }
    parameters
}

fn board_statement(shape: &BoardShape) -> String {
    let (company, cutoff) = (
        CompanyId::new(Uuid::from_u128(1)),
        DateTime::<Utc>::UNIX_EPOCH,
    );
    if !shape.cards {
        return board_reads::board_counts_query(company, &shape.scope, cutoff)
            .sql()
            .to_owned();
    }
    let visible = VisibleChannels {
        channels: vec![Uuid::from_u128(5)],
        company: true,
    };
    board_reads::board_cards_query(company, &shape.scope, cutoff, &visible)
        .sql()
        .to_owned()
}

/// Every board statement reads top-level runs only, names its states as literals (so the partial
/// indexes on them serve generic plans) and binds each scope filter once per part, in placeholder
/// order, outside the materialized working set. A column reads the cap plus one, the limit written into
/// the text.
#[test]
fn run_board_queries_bind_each_scope_filter_once_in_placeholder_order() {
    use crate::application::workflow::board::BOARD_COLUMN_CAP;
    for shape in board_shapes() {
        let sql = board_statement(&shape);
        let parameters = board_parameters(&shape);
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
        let parts = if shape.cards {
            RunStateName::ALL.len()
        } else {
            2
        };
        assert_eq!(
            sql.matches(" AND parent_run_id IS NULL").count(),
            parts,
            "{}: {sql}",
            shape.name
        );
        let scoped = shape.scope.binding.is_some()
            || shape.scope.workflow.is_some()
            || shape.scope.agent.is_some();
        let filtered = if shape.cards { parts } else { 1 };
        assert_eq!(
            sql.matches("agent_ids").count(),
            if shape.scope.agent.is_some() {
                filtered
            } else {
                0
            },
            "{}: {sql}",
            shape.name
        );
        assert_eq!(
            sql.matches(" AS MATERIALIZED (").count(),
            if shape.cards && !scoped { 0 } else { filtered },
            "{}: {sql}",
            shape.name
        );
        if shape.cards {
            for state in RunStateName::ALL {
                assert!(
                    sql.contains(&format!(" AND state = '{}'", state.as_str())),
                    "{}: {sql}",
                    shape.name
                );
            }
            let limit = format!(" LIMIT {})", BOARD_COLUMN_CAP + 1);
            assert_eq!(sql.matches(&limit).count(), parts, "{}: {sql}", shape.name);
        } else {
            assert!(
                sql.contains(
                    " AND state IN ('queued', 'running', 'waiting') AND parent_run_id IS NULL \
                     UNION ALL "
                ),
                "{}: {sql}",
                shape.name
            );
            assert!(!sql.contains("LIMIT"), "{}: {sql}", shape.name);
        }
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

/// Writes every shipped run-list and run-board shape, with its family and what each `$n` binds, as
/// JSON to the file named by `RUN_LIST_QUERY_SHAPES`, for `scripts/tests/run-list-query-plan.py`:
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
    let roles = |parameters: Vec<(&'static str, &'static str)>| -> Vec<&'static str> {
        parameters.into_iter().map(|(role, _)| role).collect()
    };
    let lists = shapes().into_iter().map(|shape| {
        serde_json::json!({
            "family": "list",
            "name": shape.name,
            "sql": statement(&shape),
            "parameters": roles(parameters(&shape)),
        })
    });
    let boards = board_shapes().into_iter().map(|shape| {
        serde_json::json!({
            "family": if shape.cards { "board_cards" } else { "board_counts" },
            "name": shape.name,
            "sql": board_statement(&shape),
            "parameters": roles(board_parameters(&shape)),
        })
    });
    let shapes: Vec<Value> = lists.chain(boards).collect();
    std::fs::write(path, serde_json::to_string_pretty(&shapes).unwrap()).unwrap();
}
