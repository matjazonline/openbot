//! The decision page and API over a decision the production engine parks.
use super::*;

/// A business decision `owner` reviews, routed `accept`/`revise` to a final map.
fn decision_source(workflow: Uuid, reviewer: Uuid) -> Value {
    let mut source = example("decision.human", workflow);
    let start = &mut source["steps"]["start"];
    start["with"]["reviewers"] = json!({"literal": [{"kind": "user", "id": reviewer}]});
    start["with"]["deadline"] =
        json!({"literal": (chrono::Utc::now() + chrono::Duration::minutes(20)).to_rfc3339()});
    start["with"]["data_schema"] = json!({"literal": {"type": "string", "maxLength": 32}});
    start["routes"]["choices"] = json!({"accept": "finish", "revise": "finish"});
    source["steps"]["finish"] = json!({"type": "data.map", "with": {"value": {"literal": 1},
        "output_schema": {"literal": {"type": "integer"}}}, "routes": {"success": "$end"}});
    source
}

/// The decision a review notice links to, through the production engine: the page and the API
/// show the decision's own choices; comments and the submission replay by key; a stale revision
/// is a conflict; a stranger is refused everywhere; the submission continues the run.
#[tokio::test]
async fn decision_routes_and_page_drive_a_parked_decision() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let (_, version) = f
        .published(f.owner, f.company, decision_source(Uuid::new_v4(), f.owner))
        .await;
    let binding = f.bound(f.owner, f.company, version).await;
    let (status, started) = f
        .start(f.owner, f.company, binding, "review", json!({}))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{started}");
    let run = uuid(&started["run_id"]);
    let decision_of = async || -> Option<Uuid> {
        sqlx::query_scalar("SELECT id FROM workflow_decisions WHERE run_id = $1")
            .bind(run)
            .fetch_optional(f.persistence.pool())
            .await
            .unwrap()
    };
    f.run_until("the decision parks", async || decision_of().await.is_some())
        .await;
    let decision = decision_of().await.unwrap();

    let (status, page) =
        answer(decisions::page(f.state(), by(f.owner), Path(decision)).await).await;
    assert_eq!(status, StatusCode::OK);
    let page = page.as_str().unwrap().to_owned();
    for expected in [r#"value="accept""#, r#"value="revise""#, "hx-disabled-elt"] {
        assert!(page.contains(expected), "the page lacks {expected}");
    }
    let (status, view) =
        answer(decisions::read(f.state(), by(f.owner), Path((f.company, decision))).await).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["choices"], json!(["accept", "revise"]), "{view}");

    let comment = |key: &str| {
        decisions::comment(
            f.state(),
            by(f.owner),
            Path((f.company, decision)),
            body(json!({"command_key": key, "body": "Looks right"})),
        )
    };
    let (status, posted) = answer(comment("n1").await).await;
    assert_eq!(
        (status, posted["replayed"].clone()),
        (StatusCode::CREATED, json!(false))
    );
    let (status, posted) = answer(comment("n1").await).await;
    assert_eq!(
        (status, posted["replayed"].clone()),
        (StatusCode::OK, json!(true))
    );
    let (status, section) = answer(
        decisions::ui_comment(
            f.state(),
            by(f.owner),
            Path(decision),
            Form(decisions::CommentForm {
                command_key: "n2".into(),
                body: "Second <look>".into(),
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let section = section.as_str().unwrap();
    assert!(section.contains("Comment posted") && section.contains("Second &lt;look&gt;"));

    let ui_submit = |user: Uuid, key: &str, data: &str| {
        decisions::ui_submit(
            f.state(),
            by(user),
            Path(decision),
            axum::body::Bytes::from(
                url::form_urlencoded::Serializer::new(String::new())
                    .extend_pairs([
                        ("command_key", key.to_owned()),
                        (
                            "expected_revision",
                            view["revision"].as_i64().unwrap().to_string(),
                        ),
                        (
                            "expected_artifact",
                            view["artifact_digest"].as_str().unwrap().to_owned(),
                        ),
                        ("choice", "accept".into()),
                        ("feedback", String::new()),
                        ("data_mode", "json".into()),
                        ("data", data.to_owned()),
                    ])
                    .finish(),
            ),
        )
    };
    let (status, refused) = answer(ui_submit(f.owner, "s0", "{not json").await).await;
    assert_eq!(status, StatusCode::OK);
    assert!(refused.as_str().unwrap().contains("not valid JSON"));

    let submit = |user: Uuid, company: Uuid, key: &str, revision: i64, choice: &str| {
        decisions::submit(
            f.state(),
            by(user),
            Path((company, decision)),
            body(json!({"command_key": key, "expected_revision": revision,
                "expected_artifact": view["artifact_digest"], "choice": choice, "data": "ok"})),
        )
    };
    let revision = view["revision"].as_i64().unwrap();
    assert_eq!(
        answer(submit(f.owner, f.company, "s1", revision + 1, "accept").await)
            .await
            .0,
        StatusCode::CONFLICT
    );
    // F058: every decision route against another tenant's decision, and a member who is not a
    // reviewer. The UI forms turn a refusal into a banner first; the follow-up read refuses too.
    let comment_as = |user: Uuid, company: Uuid| {
        decisions::comment(
            f.state(),
            by(user),
            Path((company, decision)),
            body(json!({"command_key": "x1", "body": "not mine"})),
        )
    };
    let ui_comment_as = |user: Uuid| {
        decisions::ui_comment(
            f.state(),
            by(user),
            Path(decision),
            Form(decisions::CommentForm {
                command_key: "x2".into(),
                body: "not mine".into(),
            }),
        )
    };
    for (user, company) in [
        (f.stranger, f.company),
        (f.stranger, f.other),
        (f.member, f.company),
    ] {
        assert_eq!(
            answer(submit(user, company, "s2", revision, "accept").await)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            answer(comment_as(user, company).await).await.0,
            StatusCode::NOT_FOUND
        );
    }
    for user in [f.stranger, f.member] {
        assert_eq!(
            answer(ui_submit(user, "s2", "\"ok\"").await).await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            answer(ui_comment_as(user).await).await.0,
            StatusCode::NOT_FOUND
        );
    }
    let comments: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_decision_comments WHERE decision_id = $1",
    )
    .bind(decision)
    .fetch_one(f.persistence.pool())
    .await
    .unwrap();
    assert_eq!(comments, 2, "only the owner's comments are stored");
    assert_eq!(
        answer(decisions::read(f.state(), by(f.stranger), Path((f.other, decision))).await)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        answer(decisions::page(f.state(), by(f.stranger), Path(decision)).await)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    // The page's own submission applies, and the API with the same command replays it.
    let (status, section) = answer(ui_submit(f.owner, "s3", "\"ok\"").await).await;
    assert_eq!(status, StatusCode::OK);
    let section = section.as_str().unwrap();
    assert!(
        section.contains("Decision submitted")
            && section.contains("Decided: <strong>accept</strong>"),
        "{section}"
    );
    let (status, submitted) =
        answer(submit(f.owner, f.company, "s3", revision, "accept").await).await;
    assert_eq!(
        (status, submitted["replayed"].clone()),
        (StatusCode::OK, json!(true)),
        "{submitted}"
    );
    assert_eq!(
        answer(submit(f.owner, f.company, "s3", revision, "revise").await)
            .await
            .0,
        StatusCode::CONFLICT
    );
    f.run_until("the decided run succeeds", async || {
        f.run_state(run).await == "succeeded"
    })
    .await;
    let (_, page) = answer(decisions::page(f.state(), by(f.owner), Path(decision)).await).await;
    assert!(
        page.as_str()
            .unwrap()
            .contains("Decided: <strong>accept</strong>")
    );
    assert_eq!(
        answer(decisions::page(f.state(), by(f.owner), Path(Uuid::new_v4())).await)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[path = "decision_review_tests.rs"]
mod review;
