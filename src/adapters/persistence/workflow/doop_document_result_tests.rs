use super::*;
use crate::adapters::persistence::workflow::tests::message_tests::support::member;
use crate::application::workflow::reads::WorkflowReads;
use crate::services::test_support::{LlmTurn, register_scripted_agent_base_url, scripted_llm};

fn answer() -> Vec<LlmTurn> {
    vec![LlmTurn::text(
        json!({"canvas_id":"canvas-a","status":"draft",
        "summary":"Untrusted model link [elsewhere](https://forged.invalid/)."})
        .to_string(),
    )]
}

async fn bind_params(d: &mut Document, values: Value) {
    let current =
        d.f.persistence
            .binding_summary(d.target.company, d.target.binding)
            .await
            .unwrap()
            .unwrap();
    let target = BindingTarget {
        binding: WorkflowBindingId::new(Uuid::new_v4()),
        ..d.target
    };
    let bindings = d.engine.services().bindings();
    let configured = bindings
        .configure(ConfigureBinding {
            target,
            expected: None,
            association: RelatedAssociation::Channel(d.channel),
            version: current.version,
            params: values,
            resources: BTreeMap::from([(
                ResourceName::parse("service").unwrap(),
                RuntimeResourceId::new(d.connection),
            )]),
            run_as: None,
            message_trigger: None,
        })
        .await
        .unwrap();
    bindings
        .activate(SetBindingActivity {
            target,
            expected: configured.revision,
        })
        .await
        .unwrap();
    d.target = target;
}

async fn result_output(d: &Document, scope: RunReadScope, view: &RunView) -> Option<Value> {
    let execution = view
        .detail
        .executions
        .iter()
        .find(|e| e.step.as_str() == "result")?;
    d.f.persistence
        .step_view(scope, execution.execution)
        .await
        .unwrap()
        .unwrap()
        .committed_output
}

#[derive(sqlx::FromRow)]
struct Delivered {
    body: String,
    level: i32,
    correlation_matches: bool,
    destination: Uuid,
    delivery: Uuid,
}

fn assert_notification(delivered: &Delivered, d: &Document, hostile: &str) {
    assert!(
        delivered
            .body
            .contains("[Open private document](<https://doop.example.test/c/canvas-a>)")
    );
    assert!(!delivered.body.contains("forged.invalid"));
    assert!(!delivered.body.contains(hostile));
    assert!(!delivered.body.contains("javascript:"));
    let html = crate::adapters::http::pages::render_markdown(&delivered.body);
    assert!(html.contains("href=\"https://doop.example.test/c/canvas-a\""));
    assert!(html.contains(">Open private document</a>"));
    assert!(html.contains("rel=\"noopener noreferrer\""));
    assert!(!html.contains("<img") && !html.contains("onerror") && !html.contains("<script"));
    let sent = d.delivery.transport.sent.lock().unwrap();
    // Approval notices use the same real worker. Identify the document's exact delivery.
    let notifications: Vec<_> = sent
        .iter()
        .filter(|sent| sent.id.as_uuid() == delivered.delivery)
        .collect();
    assert_eq!(
        notifications.len(),
        1,
        "one normal document delivery-worker provider call"
    );
    let wire = serde_json::to_string(&notifications[0].email).unwrap();
    assert!(wire.contains("https://doop.example.test/c/canvas-a"));
    assert!(!wire.contains("forged.invalid"));
}

async fn assert_lower_clearance_projection(d: &Document, scope: RunReadScope, view: &RunView) {
    let reader = member(&d.f, "admin").await;
    let restricted_scope = RunReadScope {
        viewer: WorkflowActor::authenticated(reader).unwrap(),
        ..scope
    };
    let restricted =
        d.f.persistence
            .run_view(restricted_scope)
            .await
            .unwrap()
            .unwrap();
    assert!(restricted.payload_restricted);
    let execution = view
        .detail
        .executions
        .iter()
        .find(|e| e.step.as_str() == "result")
        .unwrap();
    let step =
        d.f.persistence
            .step_view(restricted_scope, execution.execution)
            .await
            .unwrap()
            .unwrap();
    let output = step
        .committed_output
        .map(|value| value.to_string())
        .unwrap_or_default();
    assert!(!output.contains("https://doop.example.test"));
    assert!(!output.contains("canvas-a"));
    assert!(!output.contains("onerror"));
}

#[tokio::test]
async fn doop_result_ignores_forged_urls_and_preserves_private_message_level() {
    let d = Box::pin(Document::new(McpErrorPolicy::FailStep)).await;
    let hostile = "<img src=x onerror=alert(1)> Private draft";
    let mut created = envelope(json!({"id":"canvas-a","name":"Synthetic document",
        "url":"https://forged.invalid/create","workspace_id":"workspace-a"}));
    created["content"][0]["text"] = json!("<a href='https://forged.invalid/html'>untrusted</a>");
    d.server
        .set_tool_result(tool_name("create_canvas"), created);
    let mut final_read = final_canvas();
    final_read["structuredContent"]["canvas"]["url"] = json!("javascript:alert(1)");
    final_read["structuredContent"]["canvas"]["name"] = json!(hostile);
    d.server
        .script_tool(tool_name("get_canvas"), vec![canvas(), final_read]);
    let llm = scripted_llm(answer()).await;
    register_scripted_agent_base_url(d.agent, &llm.base_url);
    let scope = d.runtime().admit(input()).await.unwrap();
    let mut tx = d.f.persistence.pool().begin().await.unwrap();
    crate::adapters::persistence::workflow::knowledge::raise_content_level_on(
        &mut tx,
        scope.company,
        scope.run,
        crate::entities::access_level::AccessLevel::L2,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let view = d.drive(scope, |_, _| false).await;
    assert_eq!(view.detail.summary.state, "succeeded", "{view:?}");
    let output = result_output(&d, scope, &view).await.unwrap();
    assert_eq!(output["canvas_url"], "https://doop.example.test/c/canvas-a");
    assert_eq!(output["title"], hostile);
    let rendered = crate::adapters::http::pages::json_preview(&output);
    assert!(rendered.contains("&lt;img src=x onerror=alert(1)&gt; Private draft"));
    assert!(!rendered.contains("<img") && !rendered.contains("<script"));
    assert_eq!(output["status"], "draft");
    assert_eq!(output["visual_review"], "not_performed");
    let delivered = sqlx::query_as::<_, Delivered>(
        "SELECT message.clean_text_body AS body, message.access_level::int AS level,
                message.correlation_id=run.correlation_id AS correlation_matches, target.destination_id AS destination,
                target.delivery_id AS delivery
         FROM workflow_message_destinations AS target
         JOIN messages AS message ON message.company_id=target.company_id AND message.id=target.message_id
         JOIN workflow_runs AS run ON run.company_id=target.company_id AND run.id=target.run_id
         WHERE target.company_id=$1 AND target.run_id=$2",
    ).bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_all(d.f.persistence.pool()).await.unwrap();
    assert_eq!(delivered.len(), 1);
    let run_level: i32 = sqlx::query_scalar(
        "SELECT content_level::int FROM workflow_runs WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_one(d.f.persistence.pool())
    .await
    .unwrap();
    assert!(run_level >= 2);
    assert_eq!(delivered[0].level, run_level);
    assert_eq!(delivered[0].destination, d.target.actor.user_id());
    assert!(delivered[0].correlation_matches);
    assert_notification(&delivered[0], &d, hostile);
    Box::pin(assert_lower_clearance_projection(&d, scope, &view)).await;
    assert_eq!(
        d.server.call_names(),
        ["get_profile", "create_canvas", "get_canvas", "get_canvas"]
    );
    assert_eq!(llm.finish().await.unwrap(), 1);
    d.client.shutdown().await.unwrap();
}

#[tokio::test]
async fn doop_result_final_read_foreign_identity_workspace_title_or_error_cannot_deliver() {
    for fault in [
        "foreign_id",
        "foreign_workspace",
        "empty_title",
        "long_title",
        "error",
        "oversized",
    ] {
        let d = Box::pin(Document::new(McpErrorPolicy::FailStep)).await;
        let mut final_read = final_canvas();
        match fault {
            "foreign_id" => {
                final_read["structuredContent"]["canvas"]["id"] = json!("foreign-canvas")
            }
            "foreign_workspace" => {
                final_read["structuredContent"]["workspace_id"] = json!("foreign-workspace")
            }
            "empty_title" => final_read["structuredContent"]["canvas"]["name"] = json!(""),
            "long_title" => {
                final_read["structuredContent"]["canvas"]["name"] = json!("x".repeat(161))
            }
            "error" => {
                final_read =
                    json!({"content":[{"type":"text","text":"Access refused"}],"isError":true})
            }
            "oversized" => final_read["content"][0]["text"] = json!("x".repeat(16385)),
            _ => unreachable!(),
        }
        d.server
            .script_tool(tool_name("get_canvas"), vec![canvas(), final_read]);
        let llm = scripted_llm(answer()).await;
        register_scripted_agent_base_url(d.agent, &llm.base_url);
        let scope = d.runtime().admit(input()).await.unwrap();
        let view = d
            .drive(scope, |view, _| {
                fault == "oversized"
                    && view.detail.summary.waiting_reason.as_deref() == Some("reconciliation")
            })
            .await;
        assert_eq!(
            view.detail.summary.state,
            if fault == "oversized" {
                "waiting"
            } else {
                "failed"
            },
            "{fault}"
        );
        assert!(result_output(&d, scope, &view).await.is_none());
        assert!(
            !view
                .detail
                .executions
                .iter()
                .any(|e| e.step.as_str() == "notify"),
            "{fault}"
        );
        assert_eq!(
            d.server.call_names(),
            ["get_profile", "create_canvas", "get_canvas", "get_canvas"],
            "{fault}"
        );
        assert_eq!(llm.finish().await.unwrap(), 1);
        d.client.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn doop_result_invalid_created_path_segment_cannot_invoke_editor_or_deliver() {
    for id in [
        "../foreign",
        "canvas-a?share=public",
        "canvas-a#fragment",
        "//foreign.invalid",
        "%2e%2e",
        "canvas/a",
    ] {
        let d = Box::pin(Document::new(McpErrorPolicy::FailStep)).await;
        d.server.set_tool_result(tool_name("create_canvas"), envelope(json!({"id":id,
            "name":"Synthetic document","url":"https://forged.invalid","workspace_id":"workspace-a"})));
        let scope = d.runtime().admit(input()).await.unwrap();
        let view = d.drive(scope, |_, _| false).await;
        assert_eq!(view.detail.summary.state, "failed", "{id}");
        assert!(
            !view
                .detail
                .executions
                .iter()
                .any(|e| matches!(e.step.as_str(), "edit" | "notify" | "result"))
        );
        assert_eq!(d.server.call_names(), ["get_profile", "create_canvas"]);
        d.client.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn doop_result_invalid_trusted_origin_fails_before_remote_dispatch() {
    for origin in [
        "http://doop.example.test",
        "https://doop.example.test/mcp",
        "https://secret@doop.example.test",
        "https://doop.example.test/?public=1",
    ] {
        let mut d = Box::pin(Document::new(McpErrorPolicy::FailStep)).await;
        let mut values = params(&d.f);
        values["doop_origin"] = json!(origin);
        bind_params(&mut d, values).await;
        let scope = d.runtime().admit(input()).await.unwrap();
        let view = d.drive(scope, |_, _| false).await;
        assert_eq!(view.detail.summary.state, "failed", "{origin}");
        assert!(d.server.log().is_empty());
        assert!(result_output(&d, scope, &view).await.is_none());
        d.client.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn doop_result_removed_recipient_cannot_receive_or_repeat_the_known_create() {
    let mut d = Box::pin(Document::new(McpErrorPolicy::FailStep)).await;
    let recipient = member(&d.f, "member").await;
    super::delivery_support::verified_recipient(&d.f, recipient).await;
    let mut values = params(&d.f);
    values["result_destinations"] = json!([{"kind":"user","id":recipient}]);
    bind_params(&mut d, values).await;
    d.server
        .script_modes(tool_name("get_canvas"), vec![Call::Json, Call::Hang]);
    d.server
        .script_tool(tool_name("get_canvas"), vec![canvas(), final_canvas()]);
    let llm = scripted_llm(answer()).await;
    register_scripted_agent_base_url(d.agent, &llm.base_url);
    let scope = d.runtime().admit(input()).await.unwrap();
    let remove_at_final_read = async {
        tokio::time::timeout(Duration::from_secs(45), d.server.state.arrived.notified())
            .await
            .expect("the real final authenticated read must enter before recipient removal");
        let removed = sqlx::query("DELETE FROM company_members WHERE company_id=$1 AND user_id=$2")
            .bind(scope.company.as_uuid())
            .bind(recipient)
            .execute(d.f.persistence.pool())
            .await
            .unwrap();
        assert_eq!(removed.rows_affected(), 1);
        d.server.state.release.notify_one();
    };
    let (view, ()) = tokio::join!(d.drive(scope, |_, _| false), remove_at_final_read);
    assert_eq!(view.detail.summary.state, "failed", "{view:?}");
    assert!(result_output(&d, scope, &view).await.is_none());
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_message_destinations WHERE company_id=$1 AND run_id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_one(d.f.persistence.pool())
    .await
    .unwrap();
    assert_eq!(count, 0);
    assert!(
        d.delivery
            .transport
            .sent
            .lock()
            .unwrap()
            .iter()
            .all(|sent| !serde_json::to_string(&sent.email)
                .unwrap()
                .contains("https://doop.example.test/c/"))
    );
    assert_eq!(
        d.server
            .call_names()
            .iter()
            .filter(|name| name.as_str() == "get_canvas")
            .count(),
        2
    );
    assert_eq!(
        d.server
            .call_names()
            .iter()
            .filter(|name| name.as_str() == "create_canvas")
            .count(),
        1
    );
    assert_eq!(llm.finish().await.unwrap(), 1);
    d.client.shutdown().await.unwrap();
}

#[test]
fn doop_result_recipe_composition_fails_when_its_reviewed_step_bound_is_approached() {
    let source: Value = serde_json::from_str(include_str!(
        "../../../../integrations/doop/create-document.workflow.json"
    ))
    .unwrap();
    assert_eq!(
        source["steps"].as_object().unwrap().len(),
        17,
        "Review new composition before raising its budget"
    );
    assert_eq!(source["limits"]["max_steps"], 17);
    assert_eq!(source["entry"], "canonical");
    assert_eq!(
        source["steps"]["verify_final_canvas"]["routes"]["success"],
        "notify"
    );
    assert_eq!(source["steps"]["notify"]["routes"]["success"], "result");
}

#[tokio::test]
async fn doop_result_user_input_cannot_override_trusted_origin_or_recipients() {
    let d = Box::pin(Document::new(McpErrorPolicy::FailStep)).await;
    for (key, value) in [
        ("doop_origin", json!("https://forged.invalid")),
        (
            "result_destinations",
            json!([{"kind":"user","id":Uuid::new_v4()}]),
        ),
    ] {
        let mut supplied = input();
        supplied[key] = value;
        assert!(d.runtime().admit(supplied).await.is_err());
        assert!(
            d.server.log().is_empty(),
            "invalid input must never dispatch"
        );
    }
    d.client.shutdown().await.unwrap();
}

#[tokio::test]
async fn doop_result_native_one_under_step_budget_cannot_publish_success_or_repeat_create() {
    let d = Box::pin(Document::new(McpErrorPolicy::FailStep)).await;
    let llm = scripted_llm(answer()).await;
    register_scripted_agent_base_url(d.agent, &llm.base_url);
    let scope = d.runtime().admit(input()).await.unwrap();
    // Exercise the durable run budget, independently of the authored composition guard.
    let changed =
        sqlx::query("UPDATE workflow_runs SET max_steps=16 WHERE company_id=$1 AND id=$2")
            .bind(scope.company.as_uuid())
            .bind(scope.run.as_uuid())
            .execute(d.f.persistence.pool())
            .await
            .unwrap();
    assert_eq!(changed.rows_affected(), 1);
    let view = d.drive(scope, |_, _| false).await;
    assert_eq!(view.detail.summary.state, "failed", "{view:?}");
    assert!(result_output(&d, scope, &view).await.is_none());
    assert_eq!(
        d.server
            .call_names()
            .iter()
            .filter(|name| name.as_str() == "create_canvas")
            .count(),
        1
    );
    let notifications: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_message_destinations WHERE company_id=$1 AND run_id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_one(d.f.persistence.pool())
    .await
    .unwrap();
    assert!(
        notifications <= 1,
        "no duplicate document message at the step bound"
    );
    assert_eq!(llm.finish().await.unwrap(), 1);
    d.client.shutdown().await.unwrap();
}
