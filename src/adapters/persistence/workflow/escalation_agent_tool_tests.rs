//! Real private notice -> intended root -> saved model tools -> shared action/command receipts.
use super::*;
use crate::application::workflow::decisions::DecisionCommandKey;
use crate::application::workflow::escalation::{
    EscalationChoice, EscalationDecisionId, EscalationSubmission, SubmitEscalation,
    WorkflowEscalations,
};
use crate::services::test_support::{LlmTurn, register_scripted_agent_base_url, scripted_llm};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

async fn decision(f: &AgentCase) -> EscalationDecisionId {
    let id: Uuid = sqlx::query_scalar(
        "SELECT current_decision_id FROM workflow_escalation_progress WHERE company_id=$1",
    )
    .bind(f.people.role.company().as_uuid())
    .fetch_one(f.people.role.p().pool())
    .await
    .unwrap();
    EscalationDecisionId::new(id)
}

async fn state(f: &AgentCase) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('progress',progress.state, \
        'commands',(SELECT count(*) FROM workflow_escalation_commands WHERE company_id=$1), \
        'cases',(SELECT count(*) FROM workflow_escalation_cases WHERE company_id=$1), \
        'hops',(SELECT count(*) FROM workflow_run_escalations WHERE company_id=$1), \
        'runs',(SELECT count(*) FROM workflow_runs WHERE company_id=$1), \
        'handler_state',(SELECT run.state FROM workflow_escalation_handling_roots AS owned JOIN workflow_runs AS run \
            ON run.company_id=owned.company_id AND run.id=owned.run_id WHERE owned.company_id=$1), \
        'handler_jobs_active',(SELECT count(*) FROM workflow_escalation_handling_roots AS owned \
            JOIN workflow_executions AS execution ON execution.company_id=owned.company_id AND execution.run_id=owned.run_id \
            JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id \
            WHERE owned.company_id=$1 AND job.status IN ('pending','processing')), \
        'events',(SELECT jsonb_agg(event_kind ORDER BY sequence) FROM workflow_run_events WHERE company_id=$1), \
        'attempts',(SELECT jsonb_agg(jsonb_build_object('step',execution.step_id,'code',attempt.workflow_failure_code,'class',attempt.workflow_failure_class)) \
            FROM task_attempts AS attempt JOIN background_tasks AS job ON job.id=attempt.task_id \
            JOIN workflow_executions AS execution ON execution.company_id=job.company_id AND execution.id=job.workflow_execution_id WHERE job.company_id=$1), \
        'model_calls',(SELECT count(*) FROM model_usage_calls WHERE company_id=$1), \
        'actions',(SELECT jsonb_agg(jsonb_build_object('operation',intent.operation->'request'->>'operation_key','result',receipt.result)) \
            FROM workflow_action_receipts AS receipt JOIN workflow_action_intents AS intent ON intent.company_id=receipt.company_id \
              AND intent.id=receipt.invocation_id WHERE receipt.company_id=$1)) \
        FROM workflow_escalation_progress AS progress WHERE company_id=$1")
        .bind(f.people.role.company().as_uuid()).fetch_one(f.people.role.p().pool()).await.unwrap()
}

pub(super) async fn execute(f: &AgentCase) -> Value {
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_env_filter("open_bots=warn")
        .try_init();
    let p = f.people.role.p();
    assert_eq!(
        notification_worker(p, Default::default())
            .drain_once()
            .await
            .unwrap(),
        1
    );
    let engine = crate::infra::workflow::WorkflowEngine::for_tests(p.clone());
    let stop = CancellationToken::new();
    let handles = engine.spawn(&stop);
    let finished = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let row = state(f).await;
            if matches!(
                row["handler_state"].as_str(),
                Some("succeeded" | "failed" | "cancelled")
            ) && row["handler_jobs_active"] == 0
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await;
    stop.cancel();
    for (name, handle) in handles {
        tokio::time::timeout(Duration::from_secs(10), handle)
            .await
            .unwrap_or_else(|_| panic!("{name} did not stop"))
            .unwrap();
    }
    let after = state(f).await;
    assert!(finished.is_ok(), "handler did not finish: {after}");
    after
}

#[tokio::test]
async fn workflow_escalation_agent_native_read_refusal_and_explicit_ack_use_real_fenced_action_receipts()
 {
    let f = Box::pin(pending_agent_with_tools(&[
        AgentBuiltinTool::ReadEscalation,
        AgentBuiltinTool::RespondToEscalation,
    ]))
    .await;
    let id = decision(&f).await;
    let llm = scripted_llm(vec![
        LlmTurn::tool_call("read_escalation", json!({"decision_id":id})),
        LlmTurn::tool_call(
            "respond_to_escalation",
            json!({"decision_id":Uuid::new_v4(),"choice":{"kind":"acknowledge"}}),
        ),
        LlmTurn::tool_call(
            "respond_to_escalation",
            json!({"decision_id":id,"choice":{"kind":"acknowledge"}}),
        ),
        LlmTurn::text(r#"{"body":"Handled the escalation."}"#),
    ])
    .await;
    register_scripted_agent_base_url(f.created.agent.id, &llm.base_url);
    let after = Box::pin(execute(&f)).await;
    let model_result = llm.finish().await;
    assert_eq!(
        after["handler_state"], "succeeded",
        "{after}; scripted model: {model_result:?}"
    );
    assert_eq!(after["progress"], "settled");
    assert_eq!(after["commands"], 1);
    assert_eq!(after["cases"], 1);
    assert_eq!(after["hops"], 1);
    assert_eq!(
        after["runs"], 1,
        "a notice has exactly its intended handling root"
    );
    let p = f.people.role.p();
    let results: Vec<Value> = sqlx::query_scalar(
        "SELECT result FROM workflow_action_receipts WHERE company_id=$1 ORDER BY invocation_id",
    )
    .bind(f.people.role.company().as_uuid())
    .fetch_all(p.pool())
    .await
    .unwrap();
    assert_eq!(results.len(), 3, "{results:?}");
    assert_eq!(results.iter().filter(|r| r["ok"] == true).count(), 2);
    assert_eq!(results.iter().filter(|r| r["ok"] == false).count(), 1);
    let read = results
        .iter()
        .find(|r| r.get("decision").is_some())
        .unwrap();
    assert_eq!(read["decision"]["can_acknowledge"], true);
    assert!(read["decision"].get("assignment_options").is_none());
    assert_eq!(model_result.unwrap(), 4);
}

#[tokio::test]
async fn workflow_escalation_agent_native_reassign_keeps_manual_target_and_replay_after_actual_member_removal()
 {
    let f = Box::pin(pending_agent_with_tools(&[
        AgentBuiltinTool::RespondToEscalation,
    ]))
    .await;
    let id = decision(&f).await;
    let target = f.people.principals[0];
    let llm = scripted_llm(vec![
        LlmTurn::tool_call(
            "respond_to_escalation",
            json!({"decision_id":id,"choice":{"kind":"reassign","principal":target}}),
        ),
        LlmTurn::text(r#"{"body":"Reassigned the escalation."}"#),
    ])
    .await;
    register_scripted_agent_base_url(f.created.agent.id, &llm.base_url);
    let after = Box::pin(execute(&f)).await;
    let model_result = llm.finish().await;
    assert_eq!(
        after["handler_state"], "succeeded",
        "{after}; scripted model: {model_result:?}"
    );
    assert_eq!(after["progress"], "manual");
    assert_eq!(after["commands"], 1);
    assert_eq!(after["hops"], 1);
    assert_eq!(after["cases"], 1);
    let p = f.people.role.p();
    let company = f.people.role.company();
    let actor = f.people.role.admission.binding.target.actor;
    let key: String = sqlx::query_scalar(
        "SELECT command_key FROM workflow_escalation_commands WHERE company_id=$1",
    )
    .bind(company.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    let current: Uuid = sqlx::query_scalar(
        "SELECT current_decision_id FROM workflow_escalation_progress WHERE company_id=$1",
    )
    .bind(company.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    let view = p
        .read_escalation(company, actor, EscalationDecisionId::new(current))
        .await
        .unwrap();
    assert_eq!(view.recipients.len(), 1);
    assert_eq!(view.recipients[0].principal, target);
    crate::use_cases::company_invite::CompanyInvitePersistence::remove_member(
        p,
        company.as_uuid(),
        f.people.users[0],
    )
    .await
    .unwrap();
    let replay = SubmitEscalation {
        company,
        actor,
        decision: id,
        command: DecisionCommandKey::parse(key).unwrap(),
        choice: EscalationChoice::Reassign { principal: target },
    };
    assert_eq!(
        p.submit_escalation(replay).await.unwrap(),
        EscalationSubmission::Replayed
    );
    assert_eq!(state(&f).await["commands"], 1);
    assert_eq!(model_result.unwrap(), 2);
}

#[tokio::test]
async fn workflow_escalation_agent_native_capabilities_are_explicit_and_sql_aliases_cannot_forge_them()
 {
    let f = Box::pin(pending_agent()).await;
    let p = f.people.role.p();
    let company = f.people.role.company();
    let granted:i64=sqlx::query_scalar("SELECT count(*) FROM agent_capability_bindings WHERE company_id=$1 AND agent_id=$2 AND builtin IN ('read_escalation','respond_to_escalation')")
        .bind(company.as_uuid()).bind(f.created.agent.id).fetch_one(p.pool()).await.unwrap();
    assert_eq!(granted, 0, "role placement never grants escalation tools");
    for (alias, builtin, constraint) in [
        (
            "read_escalation",
            "respond_to_escalation",
            "agent_binding_reserved_alias",
        ),
        (
            "escalation.fake",
            "read_escalation.extra",
            "agent_binding_target_shape",
        ),
    ] {
        let error=sqlx::query("INSERT INTO agent_capability_bindings(company_id,id,agent_id,alias,kind,builtin) VALUES($1,$2,$3,$4,'builtin',$5)")
            .bind(company.as_uuid()).bind(Uuid::new_v4()).bind(f.created.agent.id).bind(alias).bind(builtin).execute(p.pool()).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().constraint(),
            Some(constraint)
        );
    }
}
