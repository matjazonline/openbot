//! The run board route (plan `fix-03-run-board-finished-columns`): one column per state over the
//! top-level runs the caller may see, scoped by a binding or an agent of the company, finished
//! columns windowed by when each run finished, each card counting its children.
use super::run_filter_tests::agent_source;
use super::*;
use crate::application::workflow::reads::RunStateName;
use board::BoardQuery;

impl Fixture {
    async fn board(&self, user: Uuid, company: Uuid, query: BoardQuery) -> (StatusCode, Value) {
        answer(board::board(self.state(), by(user), Path(company), Query(query)).await).await
    }

    /// `user`'s board of `company`, which must answer.
    async fn board_of(&self, user: Uuid, company: Uuid, query: BoardQuery) -> Value {
        let (status, board) = self.board(user, company, query).await;
        assert_eq!(status, StatusCode::OK, "{board}");
        board
    }

    /// Cancel `run` through the route, at the revision its detail shows.
    async fn cancelled(&self, user: Uuid, company: Uuid, run: Uuid, key: &str) {
        let (_, detail) = self.run(user, company, run).await;
        let (status, cancelled) = answer(
            bindings::cancel(
                self.state(),
                by(user),
                Path((company, run)),
                body(json!({"expected_revision": detail["revision"], "command_key": key})),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{cancelled}");
    }
}

fn column(board: &Value, state: RunStateName) -> &Value {
    board["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|column| column["state"] == state.as_str())
        .unwrap_or_else(|| panic!("no {} column: {board}", state.as_str()))
}

/// A column's `(total, card ids)`.
fn cards(board: &Value, state: RunStateName) -> (u64, Vec<Uuid>) {
    let column = column(board, state);
    (
        column["total"].as_u64().unwrap(),
        column["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| uuid(&item["run_id"]))
            .collect(),
    )
}

fn scoped(binding_id: Option<Uuid>, agent_id: Option<Uuid>) -> BoardQuery {
    BoardQuery {
        binding_id,
        agent_id,
        ..Default::default()
    }
}

/// HTTP AGENTS.md: a stranger and a plain member reach no board, and an owner naming another
/// tenant's binding or agent under their own company finds nothing. The owner's board has every
/// column in order, the default window, and the run with its children counted.
#[tokio::test]
async fn run_board_read_route_refuses_other_tenants_and_plain_members() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let ours = f.resources(f.owner, f.company).await;
    let theirs = f.resources(f.stranger, f.other).await;
    for (user, who) in [(f.stranger, "a stranger"), (f.member, "a member")] {
        let (status, board) = f.board(user, f.company, BoardQuery::default()).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{who}: {board}");
    }
    for query in [
        scoped(Some(theirs.binding), None),
        scoped(None, Some(theirs.agent)),
    ] {
        let (status, board) = f.board(f.owner, f.company, query).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{board}");
    }

    let board = f.board_of(f.owner, f.company, BoardQuery::default()).await;
    assert_eq!(board["window"], "week", "{board}");
    assert!(board["cutoff"].is_string(), "{board}");
    let states: Vec<&str> = board["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|column| column["state"].as_str().unwrap())
        .collect();
    assert_eq!(states, RunStateName::ALL.map(RunStateName::as_str));
    let queued = column(&board, RunStateName::Queued);
    assert_eq!(
        (&queued["total"], &queued["cut"]),
        (&json!(1), &json!(false))
    );
    let item = &queued["items"][0];
    assert_eq!(uuid(&item["run_id"]), ours.run, "{board}");
    assert_eq!(item["finished_at"], Value::Null, "{board}");
    assert_eq!(
        item["children"],
        json!({"total": 0, "open": 0, "failed": 0})
    );

    assert_eq!(
        cards(
            &f.board_of(f.owner, f.company, scoped(Some(ours.binding), None))
                .await,
            RunStateName::Queued
        ),
        (1, vec![ours.run])
    );
    let unused = f
        .board_of(f.owner, f.company, scoped(None, Some(ours.agent)))
        .await;
    assert!(
        unused["columns"]
            .as_array()
            .unwrap()
            .iter()
            .all(|column| column["total"] == 0),
        "no version uses the agent: {unused}"
    );
}

/// An admin's board leaves out a channel it is not allowed on, from the cards and the totals,
/// open and finished alike, and a binding on that channel is not found.
#[tokio::test]
async fn run_board_read_route_hides_channels_the_caller_cannot_see() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let (owner, company) = (f.owner, f.company);
    let (_, version) = f
        .published(owner, company, example("data.map", Uuid::new_v4()))
        .await;
    let channel = ChannelPersistence::create(
        &f.persistence,
        company,
        ChannelWrite {
            name: "Board".into(),
            slug: "board".into(),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap()
    .id;
    sqlx::query("UPDATE channels SET access_mode = 'allowlist' WHERE id = $1")
        .bind(channel)
        .execute(f.persistence.pool())
        .await
        .unwrap();
    let hidden = Uuid::new_v4();
    let (status, configured) = f
        .configure_on(
            owner,
            company,
            hidden,
            None,
            version,
            json!({"kind": "channel", "channel_id": channel}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{configured}");
    let revision = configured["state_revision"].as_u64().unwrap();
    assert_eq!(
        f.activate(owner, company, hidden, revision).await.0,
        StatusCode::OK
    );
    let shown = f.bound(owner, company, version).await;
    let mut runs = Vec::new();
    for (binding, key) in [(hidden, "hidden"), (hidden, "hidden-2"), (shown, "shown")] {
        let (status, started) = f.start(owner, company, binding, key, json!({})).await;
        assert_eq!(status, StatusCode::CREATED, "{started}");
        runs.push(uuid(&started["run_id"]));
    }
    f.cancelled(owner, company, runs[1], "cancel-hidden").await;

    let board = f.board_of(owner, company, BoardQuery::default()).await;
    assert_eq!(
        cards(&board, RunStateName::Queued),
        (2, vec![runs[0], runs[2]])
    );
    assert_eq!(cards(&board, RunStateName::Cancelled), (1, vec![runs[1]]));
    let cancelled = &column(&board, RunStateName::Cancelled)["items"][0];
    assert!(cancelled["finished_at"].is_string(), "{board}");

    let board = f.board_of(f.admin, company, BoardQuery::default()).await;
    assert_eq!(cards(&board, RunStateName::Queued), (1, vec![runs[2]]));
    assert_eq!(cards(&board, RunStateName::Cancelled), (0, Vec::new()));
    let (status, refused) = f.board(f.admin, company, scoped(Some(hidden), None)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{refused}");
    let board = f.board_of(owner, company, scoped(Some(hidden), None)).await;
    assert_eq!(cards(&board, RunStateName::Queued), (1, vec![runs[0]]));
}

/// An agent reached only through a child workflow scopes the board to the parent: the child is
/// never a card, the parent counts it, and another agent's board is empty. The child freezes the
/// agent on a branch it never takes, so the workers run the tree without calling a model.
#[tokio::test]
async fn run_board_read_route_scopes_to_an_agent_and_counts_children() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let (owner, company) = (f.owner, f.company);
    let assistant = f.model_agent(company, "assistant").await;
    let reviewer = f.model_agent(company, "reviewer").await;
    let text_input = json!({
        "type": "object",
        "properties": {"text": {"type": "string", "maxLength": 128}},
        "required": ["text"],
    });
    let child = Uuid::new_v4();
    let mut source = example("decision.rule", child);
    source["input_schema"] = text_input.clone();
    source["steps"]["start"]["routes"]["choices"]["revise"] = json!("draft");
    source["steps"]["draft"] = agent_source(Uuid::new_v4(), "assistant")["steps"]["start"].clone();
    let (_, child_version) = f.published(owner, company, source).await;
    let parent = Uuid::new_v4();
    let mut source = example("workflow.call", parent);
    source["input_schema"] = text_input;
    source["steps"]["start"]["child_workflow_id"] = json!(child.to_string());
    source["steps"]["start"]["child_version_id"] = json!(child_version.to_string());
    let (_, parent_version) = f.published(owner, company, source).await;
    let binding = f.bound(owner, company, parent_version).await;
    let root = f
        .started(owner, company, binding, "root", json!({"text": "hello"}))
        .await;
    f.run_until("the parent finishes its child", async || {
        f.run_state(root).await == "succeeded"
    })
    .await;

    let window = BoardQuery {
        agent_id: Some(assistant),
        window: crate::application::workflow::board::BoardWindow::Day,
        ..Default::default()
    };
    let board = f.board_of(owner, company, window).await;
    assert_eq!(board["window"], "day", "{board}");
    assert_eq!(cards(&board, RunStateName::Succeeded), (1, vec![root]));
    let card = &column(&board, RunStateName::Succeeded)["items"][0];
    assert!(card["finished_at"].is_string(), "{board}");
    assert_eq!(
        card["children"],
        json!({"total": 1, "open": 0, "failed": 0}),
        "{board}"
    );
    let unscoped = f.board_of(owner, company, BoardQuery::default()).await;
    assert_eq!(cards(&unscoped, RunStateName::Succeeded), (1, vec![root]));
    let other = f
        .board_of(owner, company, scoped(None, Some(reviewer)))
        .await;
    assert_eq!(cards(&other, RunStateName::Succeeded), (0, Vec::new()));
}

/// Through the run routes as mounted, with a session: `board` is the board, not a run id; a run id
/// still reads the run; and a window other than the three is refused before the handler runs.
#[tokio::test]
async fn run_board_read_route_answers_the_board_not_a_run_and_refuses_an_unknown_window() {
    use crate::adapters::http::auth::require_auth;
    use axum::{body::Body, http::Request, http::header};
    use tower::ServiceExt;
    let Some(f) = Fixture::new().await else {
        return;
    };
    let ours = f.resources(f.owner, f.company).await;
    let sessions = Arc::new(SessionAuthority::for_test(&AppConfig::for_test()));
    let app = run_routes::<Engine>()
        .route_layer(axum::middleware::from_fn_with_state(
            sessions.clone(),
            require_auth,
        ))
        .with_state(f.engine.clone());
    let cookie = format!(
        "session={}",
        sessions.cookie_for_test(f.owner).await.value()
    );
    let get = async |path: String| {
        let request = Request::get(path)
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap();
        answer(app.clone().oneshot(request).await.unwrap()).await
    };
    let runs = format!("/api/companies/{}/workflow-runs", f.company);

    let (status, board) = get(format!("{runs}/board?window=month")).await;
    assert_eq!(status, StatusCode::OK, "{board}");
    assert_eq!(board["window"], "month", "{board}");
    assert_eq!(cards(&board, RunStateName::Queued), (1, vec![ours.run]));
    let (status, run) = get(format!("{runs}/{}", ours.run)).await;
    assert_eq!(status, StatusCode::OK, "{run}");
    assert_eq!(uuid(&run["run_id"]), ours.run);
    for query in ["window=year", "window=Week", "window=week&state=failed"] {
        let (status, refused) = get(format!("{runs}/board?{query}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {refused}");
    }
}
