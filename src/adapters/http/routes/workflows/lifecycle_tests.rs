//! Drafts, publication, bindings, runs and template copies: revisions, keys and pages.
use super::*;
use crate::application::workflow::WorkflowActor;
use crate::application::workflow::http::{
    HttpConnectionWrite, HttpConnections, HttpOperation, HttpOrigin,
};
use crate::application::workflow::publication::{
    ActionEffect, ActionRecovery, ApprovedActionPolicy, ToolSnapshot,
};
use crate::application::workflow::registry::ToolContract;
use crate::domain::workflow::{CompanyId, TypeName};

/// Drafts, publication, bindings and runs: an edit carries the revision the caller saw, a create
/// under a key replays it, and the key with a different command is a conflict.
#[tokio::test]
async fn workflow_routes_carry_revisions_and_keys() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let (owner, company) = (f.owner, f.company);
    let workflow = Uuid::new_v4();
    let source = example("data.map", workflow);
    let (status, saved) = f.save(owner, company, workflow, None, &source).await;
    assert_eq!(
        (status, saved["revision"].clone()),
        (StatusCode::OK, json!(1)),
        "{saved}"
    );
    assert_eq!(
        f.save(owner, company, workflow, None, &source).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        f.save(owner, company, workflow, Some(1), &source).await.0,
        StatusCode::OK
    );
    assert_eq!(
        f.save(owner, company, workflow, Some(1), &source).await.0,
        StatusCode::CONFLICT
    );

    let mut broken = source.clone();
    broken["steps"]["start"]["routes"] = json!({"success": "missing"});
    assert_eq!(
        f.save(owner, company, workflow, Some(2), &broken).await.0,
        StatusCode::OK
    );
    let validate = |revision: u64| {
        definitions::validate(
            f.state(),
            by(owner),
            Path((company, workflow)),
            body(json!({"expected_revision": revision})),
        )
    };
    let (status, diagnostic) = answer(validate(3).await).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{diagnostic}");
    assert!(diagnostic["diagnostic"]["code"].is_string(), "{diagnostic}");
    assert_eq!(answer(validate(2).await).await.0, StatusCode::CONFLICT);
    assert_eq!(
        f.save(owner, company, workflow, Some(3), &source).await.0,
        StatusCode::OK
    );
    let (status, valid) = answer(validate(4).await).await;
    assert_eq!(
        (status, valid["valid"].clone()),
        (StatusCode::OK, json!(true)),
        "{valid}"
    );

    let (status, published) = f.publish(owner, company, workflow, 4, "p1").await;
    assert_eq!(status, StatusCode::OK, "{published}");
    let version = uuid(&published["version_id"]);
    let (status, replayed) = f.publish(owner, company, workflow, 4, "p1").await;
    assert_eq!(
        (status, uuid(&replayed["version_id"])),
        (StatusCode::OK, version)
    );
    assert_eq!(
        f.publish(owner, company, workflow, 3, "p2").await.0,
        StatusCode::CONFLICT
    );
    let (_, read) =
        answer(definitions::read(f.state(), by(owner), Path((company, workflow))).await).await;
    assert_eq!(read["versions"][0]["version_id"], json!(version), "{read}");
    assert_eq!(read["versions_truncated"], false, "{read}");

    let binding = Uuid::new_v4();
    let (status, configured) = f.configure(owner, company, binding, None, version).await;
    assert_eq!(status, StatusCode::OK, "{configured}");
    let state = configured["state_revision"].as_u64().unwrap();
    assert_eq!(
        f.configure(owner, company, binding, None, version).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        f.activate(owner, company, binding, state + 1).await.0,
        StatusCode::CONFLICT
    );
    let (status, active) = f.activate(owner, company, binding, state).await;
    assert_eq!(
        (status, active["active"].clone()),
        (StatusCode::OK, json!(true)),
        "{active}"
    );

    let (status, started) = f.start(owner, company, binding, "r1", json!({})).await;
    assert_eq!(status, StatusCode::CREATED, "{started}");
    let run = uuid(&started["run_id"]);
    let (status, again) = f.start(owner, company, binding, "r1", json!({})).await;
    assert_eq!(
        (status, uuid(&again["run_id"])),
        (StatusCode::OK, run),
        "{again}"
    );
    assert_eq!(again["replayed"], true);
    assert_eq!(
        f.start(owner, company, binding, "r1", json!({"other": 1}))
            .await
            .0,
        StatusCode::CONFLICT
    );
    let (_, listed) = answer(
        bindings::runs(
            f.state(),
            by(owner),
            Path(company),
            Query(bindings::RunsQuery {
                binding_id: Some(binding),
                ..Default::default()
            }),
        )
        .await,
    )
    .await;
    assert_eq!(listed["items"][0]["run_id"], json!(run), "{listed}");
    assert_eq!(listed["items"][0]["parent_run_id"], Value::Null, "{listed}");
    let (_, detail) = f.run(owner, company, run).await;
    assert!(
        !detail["executions"].as_array().unwrap().is_empty(),
        "{detail}"
    );
    assert_eq!(
        (
            detail["executions_truncated"].clone(),
            detail["waits_truncated"].clone()
        ),
        (json!(false), json!(false)),
        "{detail}"
    );
    let revision = detail["revision"].as_u64().unwrap();
    let cancel = |expected: u64, key: &'static str| {
        bindings::cancel(
            f.state(),
            by(owner),
            Path((company, run)),
            body(json!({"expected_revision": expected, "command_key": key})),
        )
    };
    assert_eq!(
        answer(cancel(revision + 5, "c1").await).await.0,
        StatusCode::CONFLICT
    );
    let (status, cancelled) = answer(cancel(revision, "c2").await).await;
    assert_eq!(status, StatusCode::OK, "{cancelled}");
    assert_eq!(f.run_state(run).await, "cancelled");

    let archive = |expected: u64| {
        definitions::archive(
            f.state(),
            by(owner),
            Path((company, workflow)),
            body(json!({"expected_revision": expected})),
        )
    };
    assert_eq!(answer(archive(3).await).await.0, StatusCode::CONFLICT);
    assert_eq!(answer(archive(4).await).await.0, StatusCode::NO_CONTENT);
}

/// Shipped templates copy by key: a retry returns the draft, another template under the key and
/// a stale catalogue revision are conflicts, and only an admin may copy.
#[tokio::test]
async fn template_copies_replay_by_key() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let (_, listed) =
        answer(definitions::templates(f.state(), by(f.owner), Path(f.company)).await).await;
    let templates = listed.as_array().unwrap();
    assert!(templates.len() >= 2, "{listed}");
    let copy = |user: Uuid, template: &Value, revision: u64, key: &str| {
        definitions::copy_template(
            f.state(),
            by(user),
            Path((
                f.company,
                template["template_id"].as_str().unwrap().to_owned(),
            )),
            body(json!({"revision": revision, "command_key": key})),
        )
    };
    let revision = |template: &Value| template["revision"].as_u64().unwrap();
    let (first, second) = (&templates[0], &templates[1]);
    let (status, copied) = answer(copy(f.owner, first, revision(first), "c1").await).await;
    assert_eq!(status, StatusCode::CREATED, "{copied}");
    let (status, again) = answer(copy(f.owner, first, revision(first), "c1").await).await;
    assert_eq!(
        (status, again["workflow_id"].clone()),
        (StatusCode::OK, copied["workflow_id"].clone())
    );
    assert_eq!(
        answer(copy(f.owner, second, revision(second), "c1").await)
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        answer(copy(f.owner, first, revision(first) + 1, "c2").await)
            .await
            .0,
        StatusCode::CONFLICT
    );
    let unknown = json!({"template_id": "no_such_template", "revision": 1});
    assert_eq!(
        answer(copy(f.owner, &unknown, 1, "c3").await).await.0,
        StatusCode::NOT_FOUND
    );
    for user in [f.member, f.stranger] {
        assert_eq!(
            answer(copy(user, first, revision(first), "c4").await)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
    }
    let (_, read) = answer(
        definitions::read(
            f.state(),
            by(f.owner),
            Path((f.company, uuid(&copied["workflow_id"]))),
        )
        .await,
    )
    .await;
    assert_eq!(
        read["template_origin"]["template"], first["template_id"],
        "{read}"
    );
}

/// The listing pages newest first: a page holds its size, `next` continues it, the last page has
/// none, a cursor that names no run and an out-of-range size are refused.
#[tokio::test]
async fn run_listing_pages_newest_first() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let (owner, company) = (f.owner, f.company);
    let (_, version) = f
        .published(owner, company, example("data.map", Uuid::new_v4()))
        .await;
    let binding = f.bound(owner, company, version).await;
    let mut runs = Vec::new();
    for key in ["first", "second", "third"] {
        let (_, started) = f.start(owner, company, binding, key, json!({})).await;
        runs.push(uuid(&started["run_id"]));
    }
    let page = async |after: Option<Uuid>, limit: i64| {
        answer(
            bindings::runs(
                f.state(),
                by(owner),
                Path(company),
                Query(bindings::RunsQuery {
                    binding_id: Some(binding),
                    after,
                    limit: Some(limit),
                    ..Default::default()
                }),
            )
            .await,
        )
        .await
    };
    let ids = |page: &Value| {
        Fixture::items(page)
            .0
            .iter()
            .map(|run| uuid(&run["run_id"]))
            .collect::<Vec<_>>()
    };
    let (status, first) = page(None, 2).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(ids(&first), [runs[2], runs[1]]);
    assert_eq!(Fixture::items(&first).1, Some(runs[1]));
    let (_, last) = page(Some(runs[1]), 2).await;
    assert_eq!(ids(&last), [runs[0]]);
    assert_eq!(Fixture::items(&last).1, None);
    for (after, limit) in [(Some(Uuid::new_v4()), 2), (None, 0), (None, 201)] {
        assert_eq!(page(after, limit).await.0, StatusCode::BAD_REQUEST);
    }
    let (_, definitions) = answer(
        definitions::list(
            f.state(),
            by(owner),
            Path(company),
            Query(ListQuery {
                after: None,
                limit: Some(1),
            }),
        )
        .await,
    )
    .await;
    assert_eq!(Fixture::items(&definitions).0.len(), 1, "{definitions}");
}

/// A registered HTTP operation of the company, `registered.request`, on a new connection.
async fn http_operation(f: &Fixture) -> Uuid {
    let (company, actor) = (
        CompanyId::new(f.company),
        WorkflowActor::authenticated(f.owner).unwrap(),
    );
    let connection = f
        .persistence
        .create_http(
            company,
            actor,
            HttpConnectionWrite {
                origin: HttpOrigin::parse("https://tools.example.test").unwrap(),
                bearer: false,
            },
        )
        .await
        .unwrap();
    let operation = HttpOperation {
        tool: ToolSnapshot {
            workflow: None,
            company_id: company,
            contract: ToolContract {
                name: TypeName::parse("registered.request").unwrap(),
                connection: None,
                input_schema: json!({"type": "object", "properties": {"method": {"type": "string"},
                    "path": {"type": "string"}, "headers": {"type": "object"}, "body": true},
                    "required": ["method", "path"], "additionalProperties": false}),
                output_schema: json!(true),
            },
            policy: ApprovedActionPolicy {
                capability: TypeName::parse("http.request").unwrap(),
                policy_revision: 1,
                effect: ActionEffect::Write,
                recovery: ActionRecovery::Reconcile,
            },
        },
        approval_required: false,
    };
    f.persistence
        .register_http_operation(
            company,
            actor,
            connection.id,
            connection.revision,
            operation,
        )
        .await
        .unwrap();
    connection.id
}

/// F054/F055: a draft calling a company HTTP operation validates with the selections it publishes
/// with (without them the operation is unknown), and a publish key names its selections: the key
/// again with other selections is a conflict.
#[tokio::test]
async fn validate_and_publish_carry_http_selections() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let (owner, company) = (f.owner, f.company);
    let connection = http_operation(&f).await;
    let workflow = Uuid::new_v4();
    let mut source = example("tool.call", workflow);
    source["steps"]["start"]["with"] = json!({"tool": {"literal": "registered.request"},
        "arguments": {"literal": {"method": "POST", "path": "/messages", "body": {"message": "hi"}}}});
    assert_eq!(
        f.save(owner, company, workflow, None, &source).await.0,
        StatusCode::OK
    );
    let selected = json!([{"connection": connection, "operation": "registered.request"}]);
    let validate = |http: Value| {
        definitions::validate(
            f.state(),
            by(owner),
            Path((company, workflow)),
            body(json!({"expected_revision": 1, "http": http})),
        )
    };
    let (status, unselected) = answer(validate(json!([])).await).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{unselected}");
    let (status, valid) = answer(validate(selected.clone()).await).await;
    assert_eq!(status, StatusCode::OK, "{valid}");
    let publish = |http: Value| {
        definitions::publish(
            f.state(),
            by(owner),
            Path((company, workflow)),
            body(json!({"expected_revision": 1, "command_key": "h1", "http": http})),
        )
    };
    let (status, published) = answer(publish(selected.clone()).await).await;
    assert_eq!(status, StatusCode::OK, "{published}");
    let (status, replayed) = answer(publish(selected).await).await;
    assert_eq!(
        (status, replayed["version_id"].clone()),
        (StatusCode::OK, published["version_id"].clone())
    );
    assert_eq!(
        answer(publish(json!([])).await).await.0,
        StatusCode::CONFLICT
    );
}

/// WF-C002: a runtime child is controlled through its parent. Cancelling or retrying it by its
/// own id is refused naming the parent; retrying a run that has not failed says it cannot be.
#[tokio::test]
async fn child_runs_are_controlled_through_their_parent() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let (owner, company) = (f.owner, f.company);
    let (child_workflow, child_version) = f
        .published(owner, company, example("data.map", Uuid::new_v4()))
        .await;
    let mut parent = example("workflow.call", Uuid::new_v4());
    parent["steps"]["start"]["child_workflow_id"] = json!(child_workflow.to_string());
    parent["steps"]["start"]["child_version_id"] = json!(child_version.to_string());
    let (_, version) = f.published(owner, company, parent).await;
    let binding = f.bound(owner, company, version).await;
    let (_, started) = f.start(owner, company, binding, "parent", json!({})).await;
    let root = uuid(&started["run_id"]);
    let child_of = async || -> Option<Uuid> {
        sqlx::query_scalar("SELECT id FROM workflow_runs WHERE parent_run_id = $1")
            .bind(root)
            .fetch_optional(f.persistence.pool())
            .await
            .unwrap()
    };
    f.run_until("the parent finishes its child", async || {
        child_of().await.is_some() && f.run_state(root).await == "succeeded"
    })
    .await;
    let child = child_of().await.unwrap();
    let (_, listed) = answer(
        bindings::runs(
            f.state(),
            by(owner),
            Path(company),
            Query(bindings::RunsQuery {
                binding_id: Some(binding),
                ..Default::default()
            }),
        )
        .await,
    )
    .await;
    let labelled = Fixture::items(&listed)
        .0
        .into_iter()
        .find(|run| uuid(&run["run_id"]) == child)
        .unwrap();
    assert_eq!(labelled["parent_run_id"], json!(root), "{listed}");
    for refused in [
        answer(
            bindings::cancel(
                f.state(),
                by(owner),
                Path((company, child)),
                body(json!({"expected_revision": 1, "command_key": "c"})),
            )
            .await,
        )
        .await,
        answer(
            bindings::retry(
                f.state(),
                by(owner),
                Path((company, child)),
                body(json!({"expected_revision": 1, "command_key": "r"})),
            )
            .await,
        )
        .await,
    ] {
        assert_eq!(refused.0, StatusCode::CONFLICT, "{}", refused.1);
        assert!(
            refused.1.to_string().contains(&root.to_string()),
            "{}",
            refused.1
        );
    }
    let (_, detail) = f.run(owner, company, root).await;
    let (status, refused) = answer(
        bindings::retry(
            f.state(),
            by(owner),
            Path((company, root)),
            body(json!({"expected_revision": detail["revision"], "command_key": "r2"})),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert!(
        refused.to_string().contains("cannot be retried"),
        "{refused}"
    );
    assert_eq!(
        answer(
            bindings::cancel(
                f.state(),
                by(f.stranger),
                Path((company, child)),
                body(json!({"expected_revision": 1, "command_key": "c"})),
            )
            .await,
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}
