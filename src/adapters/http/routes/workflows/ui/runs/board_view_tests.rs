//! The runs workspace's board view (plan `new-board-view` 01): six columns in state order with
//! their totals, a cut column linking to the list with the same scope, hostile titles escaped,
//! the pane beside the board, the `#run-board` region alone, and the list unchanged without
//! `view=board`.
use super::*;
use crate::application::workflow::board::BOARD_COLUMN_CAP;
use crate::application::workflow::reads::RunStateName;

fn board_query(company: Uuid) -> WorkspaceQuery {
    WorkspaceQuery {
        company_id: Some(company),
        run_id: None,
        binding_id: None,
        workflow_id: None,
        agent_id: None,
        state: None,
        view: Some("board".into()),
        window: None,
        part: None,
        reload: None,
    }
}

fn columns_query(company: Uuid) -> WorkspaceQuery {
    WorkspaceQuery {
        part: Some("columns".into()),
        ..board_query(company)
    }
}

impl Fixture {
    async fn workspace_html(&self, user: Uuid, query: WorkspaceQuery) -> AppResult<String> {
        workspace(self.ui(user), Query(query))
            .await
            .map(|html| html.0)
    }

    /// An active company binding of a published `data.map` workflow titled `title`.
    async fn data_map_binding(&self, title: &str) -> Uuid {
        let workflow = Uuid::new_v4();
        let version = self
            .published(workflow, title, &example("data.map", workflow))
            .await;
        self.bound(version, RelatedAssociation::Company).await
    }

    /// An agent of `company` with a model, which an `agent.run` step may name by `slug`.
    async fn model_agent(&self, company: Uuid, slug: &str) -> Uuid {
        use crate::use_cases::agent::{AgentPersistence, AgentWrite};
        sqlx::query(
            "INSERT INTO company_model_connections(company_id, provider, api_key, models) \
             VALUES ($1, 'openai', 'fixture-key', '{gpt-test}') ON CONFLICT DO NOTHING",
        )
        .bind(company)
        .execute(self.persistence.pool())
        .await
        .unwrap();
        AgentPersistence::create(
            &self.persistence,
            company,
            AgentWrite {
                name: slug.into(),
                slug: slug.into(),
                provider: Some("openai".into()),
                model: Some("gpt-test".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id
    }

    /// Put `run` in `state` as the engine would leave it (the database stamps `finished_at`).
    async fn drive(&self, run: Uuid, state: RunStateName) {
        sqlx::query(
            "UPDATE workflow_runs SET state = $2, \
             waiting_reason = CASE WHEN $2 = 'waiting' THEN 'timer' END WHERE id = $1",
        )
        .bind(run)
        .bind(state.as_str())
        .execute(self.persistence.pool())
        .await
        .unwrap();
    }
}

/// Another tenant's workflow, its binding, and an agent of that tenant.
struct Foreign {
    workflow: Uuid,
    binding: Uuid,
    agent: Uuid,
}

async fn foreign(f: &Fixture) -> Foreign {
    let actor = || WorkflowActor::authenticated(f.stranger).unwrap();
    let workflow = Uuid::new_v4();
    let draft = save_draft(
        &f.engine,
        f.other,
        actor(),
        workflow,
        None,
        DraftContent {
            title: "Theirs".into(),
            description: String::new(),
            source: serde_json::to_string(&example("data.map", workflow)).unwrap(),
        },
    )
    .await
    .unwrap();
    let version = publish_draft(
        &f.engine,
        f.other,
        actor(),
        workflow,
        PublishRequest {
            expected_revision: draft.revision().get(),
            command_key: "theirs",
            http: &[],
            mcp: &[],
        },
    )
    .await
    .unwrap()
    .0
    .as_uuid();
    let binding = Uuid::new_v4();
    configure_binding(
        &f.engine,
        f.other,
        actor(),
        binding,
        BindingRequest {
            expected_state_revision: None,
            association: RelatedAssociation::Company,
            version,
            params: json!({}),
            resources: BTreeMap::new(),
            run_as: None,
            message_trigger: None,
        },
    )
    .await
    .unwrap();
    Foreign {
        workflow,
        binding,
        agent: f.model_agent(f.other, "theirs").await,
    }
}

/// Each column's `(state, total)` in page order.
fn column_totals(html: &str) -> Vec<(String, u64)> {
    html.match_indices(r#"data-state=""#)
        .map(|(at, marker)| {
            let rest = &html[at + marker.len()..];
            let state = rest[..rest.find('"').unwrap()].to_owned();
            let total = rest.find("data-total>").unwrap() + "data-total>".len();
            let total = rest[total..total + rest[total..].find('<').unwrap()]
                .parse()
                .unwrap();
            (state, total)
        })
        .collect()
}

/// The six columns, in state order, each with its total of the caller's top-level runs; finished
/// columns name the window.
#[tokio::test]
async fn run_board_view_shows_six_columns_in_order_with_their_totals() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let binding = f.data_map_binding("Board").await;
    let wanted = [
        (RunStateName::Queued, 1),
        (RunStateName::Running, 1),
        (RunStateName::Waiting, 2),
        (RunStateName::Succeeded, 3),
        (RunStateName::Failed, 1),
        (RunStateName::Cancelled, 0),
    ];
    for (state, count) in wanted {
        for _ in 0..count {
            let run = f.start(binding, json!({})).await;
            f.drive(run, state).await;
        }
    }
    let html = f
        .workspace_html(f.owner, board_query(f.company))
        .await
        .unwrap();
    assert_eq!(
        column_totals(&html),
        wanted
            .iter()
            .map(|(state, count)| (state.as_str().to_owned(), *count))
            .collect::<Vec<_>>(),
        "{html}"
    );
    assert_eq!(
        html.matches("finished in the last 7 days").count(),
        3,
        "{html}"
    );
    assert!(html.contains("waiting (timer)") && html.contains("No runs."));
    assert!(
        html.contains(r#"sse-connect="/ui/workflow-runs/events?company_id="#),
        "the board keeps the run event stream on <main>"
    );
    // No run is open: the pane's column has only the empty target a card swaps into.
    assert!(
        html.contains(r#"<div id="run-pane" class="ui-pane-detail" data-pane-empty></div>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"id="run-board" class="ui-pane-list"#),
        "{html}"
    );
    // Ages: an open run from creation, a finished one from finishing.
    assert!(html.contains("· created just now") && html.contains("· finished just now"));
    let (links, unsynced) = pane_links_without_sync(&html);
    assert_eq!(
        (links, unsynced),
        (8, 0),
        "every card opens the pane, synced"
    );
}

/// A column past the cap shows the cap and says how many there are, linking to the list of that
/// state with the board's scope.
#[tokio::test]
async fn run_board_view_cut_column_links_to_the_list_with_its_scope() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let binding = f.data_map_binding("Busy").await;
    for _ in 0..=BOARD_COLUMN_CAP {
        f.start(binding, json!({})).await;
    }
    let html = f
        .workspace_html(
            f.owner,
            WorkspaceQuery {
                binding_id: Some(binding),
                ..board_query(f.company)
            },
        )
        .await
        .unwrap();
    let showing = format!(
        r#"Showing {BOARD_COLUMN_CAP} of {} · <a class="link" href="/ui/workflow-runs?company_id={}&amp;binding_id={binding}&amp;state=queued">open in list</a>"#,
        BOARD_COLUMN_CAP + 1,
        f.company,
    );
    assert!(html.contains(&showing), "{html}");
    assert_eq!(html.matches("data-cut").count(), 1);
    assert_eq!(column_totals(&html)[0], ("queued".into(), 51));
    assert_eq!(pane_links_without_sync(&html).0, BOARD_COLUMN_CAP);
    // The cards and the view switch keep the binding too.
    assert!(html.contains(&format!(
        "/ui/workflow-runs?company_id={}&amp;binding_id={binding}&amp;view=board&amp;window=week&amp;run_id=",
        f.company
    )));
    assert!(html.contains(&format!(
        r#"href="/ui/workflow-runs?company_id={}&amp;binding_id={binding}">List</a>"#,
        f.company
    )));
}

/// A workflow title is escaped where the card shows it and in its `title` attribute.
#[tokio::test]
async fn run_board_view_escapes_a_hostile_workflow_title() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let hostile = format!(r#""><{}>alert(1)</script>"#, "script");
    let binding = f.data_map_binding(&hostile).await;
    f.start(binding, json!({})).await;
    let html = f
        .workspace_html(f.owner, board_query(f.company))
        .await
        .unwrap();
    assert!(!html.contains(&hostile), "{html}");
    assert!(
        html.contains(r#"title="&quot;&gt;&lt;script&gt;alert(1)&lt;/script&gt;""#),
        "attribute: {html}"
    );
    assert!(
        html.contains("&lt;script&gt;alert(1)&lt;/script&gt;</span>"),
        "text: {html}"
    );
}

/// A plain member and another tenant's owner get no board of the company, whole or in part, and
/// an owner naming another tenant's binding finds nothing.
#[tokio::test]
async fn run_board_view_refuses_plain_members_and_other_tenants() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let binding = f.data_map_binding("Ours").await;
    let run = f.start(binding, json!({})).await;
    for user in [f.member, f.stranger] {
        let html = f
            .workspace_html(user, board_query(f.company))
            .await
            .unwrap();
        assert!(
            !html.contains(r#"id="run-board""#) && !html.contains(&run.to_string()),
            "{html}"
        );
        let part = f.workspace_html(user, columns_query(f.company)).await;
        assert!(matches!(part, Err(AppError::NotFound(_))), "{part:?}");
    }
    // The stranger's own binding, named under the owner's company.
    let theirs = foreign(&f).await.binding;
    for query in [board_query(f.company), columns_query(f.company)] {
        let named = f
            .workspace_html(
                f.owner,
                WorkspaceQuery {
                    binding_id: Some(theirs),
                    ..query
                },
            )
            .await;
        assert!(matches!(named, Err(AppError::NotFound(_))), "{named:?}");
    }
}

/// `run_id` opens the pane beside the board and highlights its card; `part=columns` answers the
/// region alone; without `view=board`, or with an unknown view, the workspace is the list.
#[tokio::test]
async fn run_board_view_opens_the_pane_answers_its_region_and_leaves_the_list_alone() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let binding = f.data_map_binding("Panes").await;
    let (open, other) = (
        f.start(binding, json!({})).await,
        f.start(binding, json!({})).await,
    );
    let html = f
        .workspace_html(
            f.owner,
            WorkspaceQuery {
                run_id: Some(open),
                ..board_query(f.company)
            },
        )
        .await
        .unwrap();
    assert!(
        html.contains(&format!(
            r#"id="run-selected" name="run_id" value="{open}""#
        )),
        "{html}"
    );
    assert!(html.contains(&format!(r#"data-run="{open}" aria-current="true""#)));
    assert!(!html.contains(&format!(r#"data-run="{other}" aria-current"#)));
    assert!(!html.contains("data-pane-empty"));

    let region = f
        .workspace_html(
            f.owner,
            WorkspaceQuery {
                run_id: Some(other),
                ..columns_query(f.company)
            },
        )
        .await
        .unwrap();
    assert!(region.starts_with(r#"<div id="run-board""#), "{region}");
    assert!(!region.contains("<main") && !region.contains("<html"));
    assert!(region.contains(&format!(r#"data-run="{other}" aria-current="true""#)));
    assert_eq!(column_totals(&region)[0], ("queued".into(), 2));
    // The protocol `scripts/tests/workflow-runs.mjs` drives: the leaf reads the toolbar's form,
    // shares the toolbar's replace group, and swaps only the columns.
    for attrs in [
        r##"hx-target="#run-board-columns" hx-select="#run-board-columns > section" hx-swap="innerHTML"
                hx-sync="#run-board:replace" hx-include="#run-selected, #run-board-filters""##,
        r##"hx-target="#run-board" hx-swap="outerHTML"
                    hx-trigger="change" hx-sync="#run-board:replace" hx-include="#run-selected" data-busy="#run-board""##,
        r#"<form id="run-board-filters""#,
        r#"<select id="run-board-window""#,
    ] {
        assert!(region.contains(attrs), "{attrs}: {region}");
    }
    assert!(
        !region.contains("window=week&amp;part=columns"),
        "the leaf names no window"
    );

    // A parent's card counts its children; the child has no card of its own.
    let (parent_version, _) = parent_and_child(&f).await;
    let parents = f.bound(parent_version, RelatedAssociation::Company).await;
    let parent = f.start(parents, json!({"note": "first"})).await;
    f.run_until_done(&[parent]).await;
    let child: Uuid = sqlx::query_scalar(
        "SELECT child_run_id FROM workflow_run_parents WHERE parent_run_id = $1",
    )
    .bind(parent)
    .fetch_one(f.persistence.pool())
    .await
    .unwrap();
    let rolled = f
        .workspace_html(f.owner, columns_query(f.company))
        .await
        .unwrap();
    assert!(
        rolled.contains(&format!(r#"data-run="{parent}""#)),
        "{rolled}"
    );
    assert!(rolled.contains("1 child run</span>"), "{rolled}");
    assert!(!rolled.contains(&format!(r#"data-run="{child}""#)));

    let list = |view: Option<&str>| WorkspaceQuery {
        view: view.map(Into::into),
        ..board_query(f.company)
    };
    let plain = f.workspace_html(f.owner, list(None)).await.unwrap();
    let unknown = f
        .workspace_html(f.owner, list(Some("kanban")))
        .await
        .unwrap();
    assert_eq!(plain, unknown);
    assert!(plain.contains(r#"id="run-list""#) && !plain.contains(r#"id="run-board""#));
    assert!(plain.contains(&format!(
        r#"href="/ui/workflow-runs?company_id={}&amp;view=board">Board</a>"#,
        f.company
    )));
}

impl Fixture {
    /// An active company binding of a published workflow whose one step runs agent `slug`.
    async fn agent_binding(&self, workflow: Uuid, slug: &str) -> Uuid {
        let mut source = example("agent.run", workflow);
        source["steps"]["start"]["with"]["agent"] = json!({"literal": slug});
        let version = self.published(workflow, "Agent flow", &source).await;
        self.bound(version, RelatedAssociation::Company).await
    }

    /// The list region a filter change reads.
    async fn list_region(&self, user: Uuid, ids: ScopeIds) -> AppResult<String> {
        list(
            self.ui(user),
            Query(ListQuery {
                company_id: self.company,
                binding_id: ids.binding,
                workflow_id: ids.workflow,
                agent_id: ids.agent,
                state: None,
                after: None,
                run_id: None,
                shown: None,
                part: None,
            }),
        )
        .await
        .map(|html| html.0)
    }
}

fn scoped(company: Uuid, ids: ScopeIds) -> WorkspaceQuery {
    WorkspaceQuery {
        binding_id: ids.binding,
        workflow_id: ids.workflow,
        agent_id: ids.agent,
        ..board_query(company)
    }
}

/// Binding, workflow and agent each keep only their runs, alone and combined, on the board and
/// in the list alike; a scope with no runs shows none.
#[tokio::test]
async fn run_board_view_filter_scopes_both_views_alone_and_combined() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let assistant = f.model_agent(f.company, "assistant").await;
    let idle = f.model_agent(f.company, "idle").await;
    let (plain, agentic) = (Uuid::new_v4(), Uuid::new_v4());
    let plain_binding = {
        let version = f
            .published(plain, "Plain", &example("data.map", plain))
            .await;
        f.bound(version, RelatedAssociation::Company).await
    };
    let agent_binding = f.agent_binding(agentic, "assistant").await;
    let plain_runs = [
        f.start(plain_binding, json!({})).await,
        f.start(plain_binding, json!({})).await,
    ];
    let agent_run = f.start(agent_binding, json!({})).await;
    let all = [plain_runs[0], plain_runs[1], agent_run];
    // The agent select offers the company's agents by name, before any is chosen.
    let unscoped = f
        .workspace_html(f.owner, board_query(f.company))
        .await
        .unwrap();
    let (first, second) = (
        unscoped
            .find(&format!(
                r#"<option value="{assistant}">assistant</option>"#
            ))
            .expect("assistant offered"),
        unscoped
            .find(&format!(r#"<option value="{idle}">idle</option>"#))
            .expect("idle offered"),
    );
    assert!(first < second, "by name");
    let ids = |binding, workflow, agent| ScopeIds {
        binding,
        workflow,
        agent,
    };
    let cases = [
        (
            ids(Some(plain_binding), None, None),
            vec![plain_runs[0], plain_runs[1]],
        ),
        (
            ids(None, Some(plain), None),
            vec![plain_runs[0], plain_runs[1]],
        ),
        (ids(None, Some(agentic), None), vec![agent_run]),
        (ids(None, None, Some(assistant)), vec![agent_run]),
        (ids(None, None, Some(idle)), vec![]),
        (ids(None, Some(agentic), Some(assistant)), vec![agent_run]),
        (ids(None, Some(plain), Some(assistant)), vec![]),
        (
            ids(Some(agent_binding), Some(agentic), Some(assistant)),
            vec![agent_run],
        ),
        (ids(Some(plain_binding), Some(agentic), None), vec![]),
    ];
    for (scope, expected) in cases {
        let board = f
            .workspace_html(f.owner, scoped(f.company, scope))
            .await
            .unwrap();
        let list = f.list_region(f.owner, scope).await.unwrap();
        for run in all {
            let wanted = expected.contains(&run);
            assert_eq!(
                board.contains(&format!(r#"data-run="{run}""#)),
                wanted,
                "board {scope:?} {run}: {board}"
            );
            assert_eq!(
                list.contains(&format!("run_id={run}")),
                wanted,
                "list {scope:?} {run}: {list}"
            );
        }
        // The chosen workflow and agent are the selected options of both views' controls.
        for (chosen, html) in [
            (scope.workflow, &board),
            (scope.agent, &board),
            (scope.workflow, &list),
            (scope.agent, &list),
        ] {
            if let Some(chosen) = chosen {
                assert!(
                    html.contains(&format!(r#"<option value="{chosen}" selected>"#)),
                    "{html}"
                );
            }
        }
    }
}

/// Another tenant's workflow, agent or binding named under the caller's company is not found on
/// either view, whole page, region or list; it is never ignored.
#[tokio::test]
async fn run_board_view_filter_refuses_another_tenants_ids() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let theirs = foreign(&f).await;
    for ids in [
        ScopeIds {
            workflow: Some(theirs.workflow),
            ..ScopeIds::default()
        },
        ScopeIds {
            agent: Some(theirs.agent),
            ..ScopeIds::default()
        },
        ScopeIds {
            binding: Some(theirs.binding),
            ..ScopeIds::default()
        },
    ] {
        for query in [
            scoped(f.company, ids),
            WorkspaceQuery {
                part: Some("columns".into()),
                ..scoped(f.company, ids)
            },
            WorkspaceQuery {
                view: None,
                ..scoped(f.company, ids)
            },
        ] {
            let answer = f.workspace_html(f.owner, query).await;
            assert!(
                matches!(answer, Err(AppError::NotFound(_))),
                "{ids:?}: {answer:?}"
            );
        }
        let listed = f.list_region(f.owner, ids).await;
        assert!(
            matches!(listed, Err(AppError::NotFound(_))),
            "{ids:?}: {listed:?}"
        );
    }
}

/// The List/Board switch, the list's reload and "any binding" keep the scope; the board's reload
/// leaf reads the toolbar's form, which carries it. The agent pane's "Used by" card links to the
/// agent's board, and that board opens scoped to the agent.
#[tokio::test]
async fn run_board_view_filter_switch_and_links_keep_the_scope() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let assistant = f.model_agent(f.company, "assistant").await;
    let workflow = Uuid::new_v4();
    let binding = f.agent_binding(workflow, "assistant").await;
    let run = f.start(binding, json!({})).await;
    let ids = ScopeIds {
        binding: Some(binding),
        workflow: Some(workflow),
        agent: Some(assistant),
    };
    let scope = format!(
        "company_id={}&amp;binding_id={binding}&amp;workflow_id={workflow}&amp;agent_id={assistant}",
        f.company
    );
    let board = f
        .workspace_html(f.owner, scoped(f.company, ids))
        .await
        .unwrap();
    assert!(
        board.contains(&format!(r#"href="/ui/workflow-runs?{scope}">List</a>"#)),
        "{board}"
    );
    assert!(board.contains(&format!(
        r#"href="/ui/workflow-runs?{scope}&amp;view=board&amp;window=week&amp;run_id={run}""#
    )));
    assert!(board.contains(&format!(
        r#"<input type="hidden" name="binding_id" value="{binding}">"#
    )));
    let list = f
        .workspace_html(
            f.owner,
            WorkspaceQuery {
                view: None,
                ..scoped(f.company, ids)
            },
        )
        .await
        .unwrap();
    assert!(
        list.contains(&format!(
            r#"href="/ui/workflow-runs?{scope}&amp;view=board">Board</a>"#
        )),
        "{list}"
    );
    // The list's reload reads the filter form as it is now, which carries the scope.
    assert!(list.contains(
        r##"hx-get="/ui/workflow-runs/list?part=entries" hx-trigger="sse:runs, sse:resync, run-selected from:body"
                hx-target="#run-list-entries" hx-swap="innerHTML" hx-sync="#run-list:replace"
                hx-include="#run-list-shown, #run-selected, #run-list-filters""##
    ));
    assert!(list.contains(r#"<form id="run-list-filters""#));
    assert!(list.contains(&format!(
        r#"href="/ui/workflow-runs?company_id={}&amp;workflow_id={workflow}&amp;agent_id={assistant}">any binding</a>"#,
        f.company
    )));

    let card = pages::agent_usage_card(&pages::AgentUsageCard {
        company_id: f.company,
        agent: assistant,
        bindings: &[],
        hidden: 0,
        truncated: false,
    });
    let link = format!(
        "/ui/workflow-runs?company_id={}&amp;view=board&amp;agent_id={assistant}",
        f.company
    );
    assert!(card.contains(&link), "{card}");
    let agent_board = f
        .workspace_html(
            f.owner,
            scoped(
                f.company,
                ScopeIds {
                    agent: Some(assistant),
                    ..ScopeIds::default()
                },
            ),
        )
        .await
        .unwrap();
    assert!(agent_board.contains(&format!(r#"<option value="{assistant}" selected>"#)));
    assert!(agent_board.contains(&format!(r#"data-run="{run}""#)));
}

/// An option list past its bound says so rather than dropping options silently, and a chosen id
/// past the bound stays selected.
#[test]
fn run_board_view_filter_reports_a_cut_option_list() {
    let option = |label: &str| pages::ScopeOption {
        id: Uuid::new_v4(),
        label: label.into(),
    };
    let hostile = format!("<{}>x", "script");
    let options = pages::ScopeOptions {
        workflows: vec![option("First"), option(&hostile)],
        workflows_cut: true,
        agents: vec![option("Assistant")],
        agents_cut: false,
    };
    let beyond = Uuid::new_v4();
    let html = pages::scope_controls(
        Uuid::new_v4(),
        pages::RunFilterView {
            workflow: Some(beyond),
            ..Default::default()
        },
        &options,
        pages::RunsView::Board,
    );
    assert!(
        html.contains("Only the first 2 workflows are offered."),
        "{html}"
    );
    assert!(!html.contains("agents are offered"), "{html}");
    assert!(
        html.contains(&format!(r#"<option value="{beyond}" selected>"#)),
        "{html}"
    );
    assert!(
        !html.contains(&hostile) && html.contains("&lt;script&gt;x"),
        "{html}"
    );
}

/// The column `run`'s card sits in, by the `data-state` of the section before it.
fn column_of(html: &str, run: Uuid) -> Option<String> {
    let at = html.find(&format!(r#"data-run="{run}""#))?;
    let marker = r#"data-state=""#;
    let start = html[..at].rfind(marker)? + marker.len();
    Some(html[start..start + html[start..].find('"').unwrap()].to_owned())
}

/// The stream's next event as SSE text within `wait`, `None` when nothing came.
async fn within(
    stream: &mut (
             impl tokio_stream::Stream<
        Item = Result<axum::response::sse::Event, std::convert::Infallible>,
    > + Unpin
         ),
    wait: Duration,
) -> Option<String> {
    tokio::time::timeout(wait, stream.next())
        .await
        .ok()
        .flatten()
        .map(|event| format!("{:?}", event.unwrap()))
}

/// A wake-up's read of the board's columns, as the leaf sends it.
fn reload(company: Uuid) -> WorkspaceQuery {
    WorkspaceQuery {
        reload: Some("columns".into()),
        ..columns_query(company)
    }
}

/// HTTP AGENTS.md: a run that changes between the page's render and the stream's subscription is
/// reconciled by the stream's first `resync`, which the board's leaf reads on.
#[tokio::test]
async fn run_board_view_live_reconciles_a_change_before_subscribing_on_the_first_resync() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let binding = f.data_map_binding("Live").await;
    let run = f.start(binding, json!({})).await;
    let page = f
        .workspace_html(f.owner, board_query(f.company))
        .await
        .unwrap();
    assert_eq!(column_of(&page, run).as_deref(), Some("queued"));
    assert!(
        page.contains(
            r#"hx-trigger="sse:runs, sse:resync, sse:tick, run-selected from:body, board-wake""#
        ),
        "{page}"
    );
    // Between the render and the subscription, the run fails; nothing announces it to a stream
    // that is not yet listening.
    f.drive(run, RunStateName::Failed).await;
    let events = MailboxEvents::new();
    let stream = events::run_event_stream(
        events.subscribe(),
        f.ui(f.owner),
        f.company,
        Duration::from_secs(3600),
    );
    tokio::pin!(stream);
    let first = within(&mut stream, Duration::from_secs(5)).await.unwrap();
    assert!(first.contains("resync"), "{first}");
    let read = f.workspace_html(f.owner, reload(f.company)).await.unwrap();
    assert_eq!(column_of(&read, run).as_deref(), Some("failed"), "{read}");
}

/// A run on a channel the viewer cannot see wakes nothing, and is on no board of theirs; a run
/// they can see wakes `runs`.
#[tokio::test]
async fn run_board_view_live_sends_no_wake_up_for_a_hidden_channel() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let channel = f.channel("Private", "private", true).await;
    let workflow = Uuid::new_v4();
    let version = f
        .published(workflow, "Hidden", &example("data.map", workflow))
        .await;
    let hidden = f
        .bound(
            version,
            RelatedAssociation::Channel(RelatedChannelId::new(channel)),
        )
        .await;
    let run = f.start(hidden, json!({})).await;
    let events = MailboxEvents::new();
    let stream = events::run_event_stream(
        events.subscribe(),
        f.ui(f.admin),
        f.company,
        Duration::from_secs(3600),
    );
    tokio::pin!(stream);
    assert!(
        within(&mut stream, Duration::from_secs(5))
            .await
            .unwrap()
            .contains("resync")
    );
    events.publish(MailboxEvent::WorkflowRunChanged(RunScope {
        company_id: f.company,
        channel_id: Some(channel),
        thread_id: None,
        run_id: run,
    }));
    assert_eq!(within(&mut stream, Duration::from_millis(300)).await, None);
    let board = f.workspace_html(f.admin, reload(f.company)).await.unwrap();
    assert_eq!(column_of(&board, run), None, "{board}");
    // The control: a company-wide run the admin can see does wake the board.
    let seen = Uuid::new_v4();
    events.publish(MailboxEvent::WorkflowRunChanged(RunScope {
        company_id: f.company,
        channel_id: None,
        thread_id: None,
        run_id: seen,
    }));
    let woke = within(&mut stream, Duration::from_secs(5)).await.unwrap();
    assert!(
        woke.contains("runs") && woke.contains(&seen.to_string()),
        "{woke}"
    );
}

/// The window's cutoff is recomputed on every read: a run finished now is on the week's board, and
/// a read eight days later leaves it out, while the month's still shows it, aged from finishing.
#[tokio::test]
async fn run_board_view_live_moves_a_finished_run_out_of_the_window_on_a_later_read() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let binding = f.data_map_binding("Window").await;
    let run = f.start(binding, json!({})).await;
    f.drive(run, RunStateName::Succeeded).await;
    let at = |as_of, window| {
        let ui = f.ui(f.owner);
        async move {
            ui.board_html_at(
                f.company,
                ScopeIds::default(),
                window,
                None,
                BoardRead::Columns,
                as_of,
            )
            .await
            .unwrap()
        }
    };
    let now = chrono::Utc::now();
    let later = now + chrono::Duration::days(8);
    let week = at(now, BoardWindow::Week).await;
    assert_eq!(
        column_of(&week, run).as_deref(),
        Some("succeeded"),
        "{week}"
    );
    assert!(week.contains("finished just now"));
    let next_week = at(later, BoardWindow::Week).await;
    assert_eq!(column_of(&next_week, run), None, "{next_week}");
    let month = at(later, BoardWindow::Month).await;
    assert_eq!(column_of(&month, run).as_deref(), Some("succeeded"));
    assert!(month.contains("finished 8d ago"), "{month}");
}
