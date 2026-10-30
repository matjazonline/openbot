//! The run list's agent filter (plan `fix-01-run-agent-query`): runs of versions whose
//! `agent_ids` display index holds the agent — through the version each run started on, and
//! through a pinned child — combined with the other filters and the cursor, and only ever for an
//! agent of the company being listed.
use super::*;

impl Fixture {
    /// An agent of `company` on an enabled model connection, as publication requires.
    async fn model_agent(&self, company: Uuid, slug: &str) -> Uuid {
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

    /// `source` saved over draft `revision` of `workflow` and published: the new version.
    async fn republished(
        &self,
        owner: Uuid,
        company: Uuid,
        workflow: Uuid,
        revision: u64,
        source: &Value,
    ) -> Uuid {
        let (status, saved) = self
            .save(owner, company, workflow, Some(revision), source)
            .await;
        assert_eq!(status, StatusCode::OK, "{saved}");
        let key = format!("republish-{}-{revision}", workflow.simple());
        let (status, published) = self
            .publish(owner, company, workflow, revision + 1, &key)
            .await;
        assert_eq!(status, StatusCode::OK, "{published}");
        uuid(&published["version_id"])
    }

    /// A run started on `binding` under `key`: its id.
    async fn started(
        &self,
        owner: Uuid,
        company: Uuid,
        binding: Uuid,
        key: &str,
        input: Value,
    ) -> Uuid {
        let (status, started) = self.start(owner, company, binding, key, input).await;
        assert_eq!(status, StatusCode::CREATED, "{started}");
        uuid(&started["run_id"])
    }

    async fn runs_page(&self, user: Uuid, company: Uuid, query: RunsQuery) -> (StatusCode, Value) {
        answer(bindings::runs(self.state(), by(user), Path(company), Query(query)).await).await
    }

    /// Every run id one listing returns, newest first; it must fit one page.
    async fn listed(&self, user: Uuid, company: Uuid, query: RunsQuery) -> Vec<Uuid> {
        let (status, page) = self.runs_page(user, company, query).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        let (items, next) = Fixture::items(&page);
        assert_eq!(next, None, "{page}");
        items.iter().map(|item| uuid(&item["run_id"])).collect()
    }
}

use bindings::RunsQuery;

/// `example("agent.run")` with its agent handle replaced by `slug`.
fn agent_source(workflow: Uuid, slug: &str) -> Value {
    let mut source = example("agent.run", workflow);
    source["steps"]["start"]["with"]["agent"] = json!({"literal": slug});
    source
}

fn by_agent(agent: Uuid) -> RunsQuery {
    RunsQuery {
        agent_id: Some(agent),
        ..Default::default()
    }
}

/// A run belongs to the version it started on: re-pointing its binding at a version without the
/// agent leaves earlier runs matching and later ones not, and another agent's runs never match.
#[tokio::test]
async fn run_agent_filter_follows_the_version_each_run_started_on() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let (owner, company) = (f.owner, f.company);
    let assistant = f.model_agent(company, "assistant").await;
    let reviewer = f.model_agent(company, "reviewer").await;
    let workflow = Uuid::new_v4();
    let (_, with_agent) = f
        .published(owner, company, agent_source(workflow, "assistant"))
        .await;
    let without_agent = f
        .republished(owner, company, workflow, 1, &example("data.map", workflow))
        .await;

    let binding = Uuid::new_v4();
    let (status, configured) = f.configure(owner, company, binding, None, with_agent).await;
    assert_eq!(status, StatusCode::OK, "{configured}");
    let (status, active) = f
        .activate(
            owner,
            company,
            binding,
            configured["state_revision"].as_u64().unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{active}");
    let first = f.started(owner, company, binding, "first", json!({})).await;
    let (status, repointed) = f
        .configure(
            owner,
            company,
            binding,
            active["state_revision"].as_u64(),
            without_agent,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{repointed}");
    assert_eq!(repointed["active"], json!(true), "{repointed}");
    let second = f
        .started(owner, company, binding, "second", json!({}))
        .await;

    let other = Uuid::new_v4();
    let (_, reviewed) = f
        .published(owner, company, agent_source(other, "reviewer"))
        .await;
    let reviewed_binding = f.bound(owner, company, reviewed).await;
    let third = f
        .started(owner, company, reviewed_binding, "third", json!({}))
        .await;

    assert_eq!(f.listed(owner, company, by_agent(assistant)).await, [first]);
    assert_eq!(f.listed(owner, company, by_agent(reviewer)).await, [third]);
    assert_eq!(
        f.listed(owner, company, RunsQuery::default()).await,
        [third, second, first]
    );
}

/// An agent reached only through a pinned child workflow is in the parent version's index, so the
/// parent's runs match; a plain workflow's do not.
#[tokio::test]
async fn run_agent_filter_matches_a_parent_through_its_pinned_child() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let (owner, company) = (f.owner, f.company);
    let assistant = f.model_agent(company, "assistant").await;
    let text_input = json!({"type": "object", "properties": {"text": {"type": "string", "maxLength": 128}}, "required": ["text"]});

    let child = Uuid::new_v4();
    let mut source = agent_source(child, "assistant");
    source["input_schema"] = text_input.clone();
    let (_, child_version) = f.published(owner, company, source).await;

    let parent = Uuid::new_v4();
    let mut source = example("workflow.call", parent);
    source["input_schema"] = text_input;
    source["steps"]["start"]["child_workflow_id"] = json!(child.to_string());
    source["steps"]["start"]["child_version_id"] = json!(child_version.to_string());
    let (_, parent_version) = f.published(owner, company, source).await;
    let parent_binding = f.bound(owner, company, parent_version).await;
    let run = f
        .started(
            owner,
            company,
            parent_binding,
            "parent",
            json!({"text": "hello"}),
        )
        .await;

    let plain = Uuid::new_v4();
    let (_, plain_version) = f
        .published(owner, company, example("data.map", plain))
        .await;
    let plain_binding = f.bound(owner, company, plain_version).await;
    f.started(owner, company, plain_binding, "plain", json!({}))
        .await;

    assert_eq!(f.listed(owner, company, by_agent(assistant)).await, [run]);
}

/// The agent filter narrows the binding and state filters rather than replacing them, and its
/// pages stay full and in order across the cursor.
#[tokio::test]
async fn run_agent_filter_combines_with_binding_state_and_the_cursor() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let (owner, company) = (f.owner, f.company);
    let assistant = f.model_agent(company, "assistant").await;
    let mut bindings = Vec::new();
    for source in [
        agent_source(Uuid::new_v4(), "assistant"),
        agent_source(Uuid::new_v4(), "assistant"),
        example("data.map", Uuid::new_v4()),
    ] {
        let (_, version) = f.published(owner, company, source).await;
        bindings.push(f.bound(owner, company, version).await);
    }
    let (a, b, plain) = (bindings[0], bindings[1], bindings[2]);
    let a1 = f.started(owner, company, a, "a1", json!({})).await;
    let b1 = f.started(owner, company, b, "b1", json!({})).await;
    f.started(owner, company, plain, "c1", json!({})).await;
    let a2 = f.started(owner, company, a, "a2", json!({})).await;
    let b2 = f.started(owner, company, b, "b2", json!({})).await;

    let (_, detail) = f.run(owner, company, b1).await;
    let (status, cancelled) = answer(
        bindings::cancel(
            f.state(),
            by(owner),
            Path((company, b1)),
            body(json!({"expected_revision": detail["revision"], "command_key": "cancel-b1"})),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{cancelled}");

    assert_eq!(
        f.listed(owner, company, by_agent(assistant)).await,
        [b2, a2, b1, a1]
    );
    let narrowed = |binding: Option<Uuid>, state: Option<&str>| RunsQuery {
        binding_id: binding,
        state: state.map(str::to_owned),
        ..by_agent(assistant)
    };
    assert_eq!(
        f.listed(owner, company, narrowed(Some(a), None)).await,
        [a2, a1]
    );
    assert!(
        f.listed(owner, company, narrowed(Some(plain), None))
            .await
            .is_empty()
    );
    assert_eq!(
        f.listed(owner, company, narrowed(None, Some("queued")))
            .await,
        [b2, a2, a1]
    );
    assert_eq!(
        f.listed(owner, company, narrowed(None, Some("cancelled")))
            .await,
        [b1]
    );

    let mut paged = Vec::new();
    let mut after = None;
    loop {
        let (status, page) = f
            .runs_page(
                owner,
                company,
                RunsQuery {
                    after,
                    limit: Some(1),
                    ..by_agent(assistant)
                },
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{page}");
        let (items, next) = Fixture::items(&page);
        assert_eq!(items.len(), 1, "every page is full: {page}");
        paged.push(uuid(&items[0]["run_id"]));
        match next {
            Some(next) => after = Some(next),
            None => break,
        }
    }
    assert_eq!(paged, [b2, a2, b1, a1]);
}

/// The agent is resolved inside the company being listed: another company's agent, or a deleted
/// one, is not found rather than an empty page, while an agent no version uses lists nothing. A
/// caller who may not list the company's runs is refused before the agent is looked at.
#[tokio::test]
async fn run_agent_filter_finds_only_an_agent_of_the_company() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let (owner, company) = (f.owner, f.company);
    let mine = f.model_agent(company, "assistant").await;
    let theirs = f.model_agent(f.other, "assistant").await;
    let (_, version) = f
        .published(owner, company, agent_source(Uuid::new_v4(), "assistant"))
        .await;
    let binding = f.bound(owner, company, version).await;
    let run = f.started(owner, company, binding, "mine", json!({})).await;
    let (_, their_version) = f
        .published(
            f.stranger,
            f.other,
            agent_source(Uuid::new_v4(), "assistant"),
        )
        .await;
    let their_binding = f.bound(f.stranger, f.other, their_version).await;
    f.started(f.stranger, f.other, their_binding, "theirs", json!({}))
        .await;

    assert_eq!(f.listed(owner, company, by_agent(mine)).await, [run]);
    let (status, page) = f.runs_page(owner, company, by_agent(theirs)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{page}");
    let (status, page) = f.runs_page(f.stranger, f.other, by_agent(mine)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{page}");

    let idle = f.model_agent(company, "idle").await;
    assert!(f.listed(owner, company, by_agent(idle)).await.is_empty());
    AgentPersistence::delete(&f.persistence, idle)
        .await
        .unwrap();
    let (status, page) = f.runs_page(owner, company, by_agent(idle)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{page}");

    let (unfiltered, _) = f.runs_page(f.member, company, RunsQuery::default()).await;
    assert_ne!(unfiltered, StatusCode::OK);
    let (filtered, page) = f.runs_page(f.member, company, by_agent(mine)).await;
    assert_eq!(filtered, unfiltered, "{page}");
    let (filtered, page) = f.runs_page(f.stranger, company, by_agent(mine)).await;
    assert_ne!(filtered, StatusCode::OK, "{page}");
}
