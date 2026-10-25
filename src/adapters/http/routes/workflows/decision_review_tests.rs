//! A real two-round review journey, including raw invalid input and exact late links.
use super::*;

fn child_source(reviewer: Uuid) -> Value {
    let mut source = decision_source(Uuid::new_v4(), reviewer);
    source["input_schema"] = json!({"type":"object","properties":{"body":{"type":"string","maxLength":32},"feedback":{"type":"string","maxLength":16384}},"required":["body","feedback"],"additionalProperties":false});
    source["output_schema"] = json!({"type":"object","properties":{"choice":{"type":"string","enum":["accept","revise"]},"feedback":{"type":"string"},"artifact":{"type":"object","properties":{"body":{"type":"string","maxLength":32}},"required":["body"],"additionalProperties":false}},"required":["choice","feedback","artifact"],"additionalProperties":false});
    source["steps"]["start"]["with"]["proposal"] = json!({"object":{"body":{"ref":"/input/body"}}});
    source["steps"]["start"]["with"]["data_schema"] = json!({"literal":{"type":"object","properties":{"body":{"type":"string","maxLength":32}},"required":["body"],"additionalProperties":false}});
    source["steps"]["start"]["with"]["feedback_required"] = json!({"literal":["revise"]});
    source["steps"]["finish"]["with"] = json!({"value":{"object":{
        "choice":{"ref":"/steps/start/output/choice"},"feedback":{"ref":"/steps/start/output/feedback"},
        "artifact":{"ref":"/steps/start/output/data"}}},"output_schema":{"literal":true}});
    source["steps"]["finish"]["with"]["output_schema"] =
        json!({"literal":source["output_schema"].clone()});
    source
}

fn parent_source(child: Uuid, version: Uuid) -> Value {
    let mut source = example("flow.repeat", Uuid::new_v4());
    source["output_schema"] = json!(true);
    let start = &mut source["steps"]["start"];
    start["child_workflow_id"] = json!(child);
    start["child_version_id"] = json!(version);
    start["max_iterations"] = json!(2);
    start["with"] = json!({"input":{"literal":{"body":"Draft one","feedback":""}}});
    start["repeat"] = json!({"next_input":{"object":{
        "body":{"ref":"/steps/start/output/result/artifact/body"},
        "feedback":{"ref":"/steps/start/output/result/feedback"}}},
        "exit_when":{"ne":[{"ref":"/steps/start/output/result/choice"},{"literal":"revise"}]}});
    source
}

async fn decision_ids(f: &Fixture, parent: Uuid) -> Vec<Uuid> {
    sqlx::query_scalar("SELECT decision.id FROM workflow_decisions AS decision JOIN workflow_runs AS run ON run.id=decision.run_id WHERE run.parent_run_id=$1 ORDER BY decision.created_at,decision.id")
        .bind(parent).fetch_all(f.persistence.pool()).await.unwrap()
}
async fn view(f: &Fixture, decision: Uuid) -> Value {
    let (status, view) =
        answer(decisions::read(f.state(), by(f.owner), Path((f.company, decision))).await).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    view
}
async fn submit_fields(
    f: &Fixture,
    view: &Value,
    choice: &str,
    feedback: &str,
    data: &str,
) -> String {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("command_key", Uuid::new_v4().to_string()),
            ("expected_revision", view["revision"].to_string()),
            (
                "expected_artifact",
                view["artifact_digest"].as_str().unwrap().to_owned(),
            ),
            ("choice", choice.into()),
            ("feedback", feedback.into()),
            ("data_mode", "fields".into()),
            ("data:body", data.into()),
        ])
        .finish();
    let (status, page) = answer(
        decisions::ui_submit(
            f.state(),
            by(f.owner),
            Path(uuid(&view["decision_id"])),
            axum::body::Bytes::from(body),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    page.as_str().unwrap().into()
}

#[tokio::test]
async fn reviewer_comments_revises_reads_next_draft_and_accepts_exact_artifact() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let parent = start_review(&f).await;
    let first = decision_ids(&f, parent).await[0];
    let first_view = view(&f, first).await;
    assert_eq!(first_view["proposal"], json!({"body":"Draft one"}));
    assert_eq!(first_view["subject"], "business");
    assert_eq!(first_view["feedback_required"], json!(["revise"]));
    comment_on_first(&f, first).await;
    let refused = submit_fields(&f, &first_view, "revise", "", "Draft two").await;
    assert!(
        refused.contains("Draft two") && !refused.contains("Decision submitted"),
        "{refused}"
    );
    assert_eq!(view(&f, first).await["state"], "waiting");
    let accepted = submit_fields(&f, &first_view, "revise", "Shorter <please>", "Draft two").await;
    assert!(accepted.contains("Decision submitted"), "{accepted}");
    f.run_until("second review", async || {
        decision_ids(&f, parent).await.len() == 2
    })
    .await;
    let second = decision_ids(&f, parent).await[1];
    let second_view = view(&f, second).await;
    assert_eq!(second_view["proposal"], json!({"body":"Draft two"}));
    assert_ne!(
        first_view["artifact_digest"],
        second_view["artifact_digest"]
    );
    let history = second_view["history"].as_array().unwrap();
    assert_eq!(history.len(), 2, "{second_view}");
    assert_eq!(history[0]["round"], 1);
    assert_eq!(history[0]["feedback"], "Shorter <please>");
    assert_eq!(history[1]["round"], 2);
    assert_eq!(history[1]["decision_id"], second.to_string());
    let (_, page) = answer(decisions::page(f.state(), by(f.owner), Path(second)).await).await;
    let page = page.as_str().unwrap();
    assert!(page.contains("Shorter &lt;please&gt;"));
    assert!(page.contains("name=\"data:body\""));
    let stale = submit_fields(&f, &first_view, "accept", "late", "Wrong draft").await;
    assert!(
        stale.contains("already") || stale.contains("closed") || stale.contains("changed"),
        "{stale}"
    );
    assert!(!stale.contains("/submit\""));
    assert_eq!(view(&f, second).await["state"], "waiting");
    assert!(
        submit_fields(&f, &second_view, "accept", "", "Final draft")
            .await
            .contains("Decision submitted")
    );
    f.run_until("parent completed", async || {
        sqlx::query_scalar::<_, String>("SELECT state FROM workflow_runs WHERE id=$1")
            .bind(parent)
            .fetch_one(f.persistence.pool())
            .await
            .unwrap()
            == "succeeded"
    })
    .await;
    assert_eq!(view(&f, second).await["can_submit"], false);
}

async fn start_review(f: &Fixture) -> Uuid {
    let (child, version) = f.published(f.owner, f.company, child_source(f.owner)).await;
    let (_, version) = f
        .published(f.owner, f.company, parent_source(child, version))
        .await;
    let binding = f.bound(f.owner, f.company, version).await;
    let (status, started) = f
        .start(
            f.owner,
            f.company,
            binding,
            "review-rounds",
            json!({"text":"review"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{started}");
    let parent = uuid(&started["run_id"]);
    f.run_until("first review", async || {
        decision_ids(f, parent).await.len() == 1
    })
    .await;
    parent
}

async fn comment_on_first(f: &Fixture, first: Uuid) {
    let (status, _) = answer(
        decisions::ui_comment(
            f.state(),
            by(f.owner),
            Path(first),
            Form(decisions::CommentForm {
                command_key: "comment".into(),
                body: "Please inspect <this>".into(),
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view(f, first).await["state"], "waiting");
}
