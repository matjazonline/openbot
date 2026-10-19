//! Assertions shared by production dispatch, repair, approval-resume and cancellation scenarios.
use super::*;
use crate::services::{
    harness::runs::RunCheckpoint,
    model_usage::{ModelUsageStore, UsageGrouping, UsageQuery, UsageScope},
    test_support::ScriptedResponse,
};
use serde_json::Value;

pub(super) fn partial_response(answer: &str) -> ScriptedResponse {
    let response = ScriptedResponse::turn(LlmTurn::text(answer), 1);
    let mut body: Value = serde_json::from_str(&response.body).unwrap();
    body["usage"] = serde_json::json!({"prompt_tokens": 2});
    ScriptedResponse::json(body)
}

pub(super) async fn assert_projection(fx: &Fixture, run: &RunCheckpoint) {
    let rows: Vec<Value> = sqlx::query_scalar(
        "SELECT to_jsonb(usage) FROM model_usage_calls AS usage WHERE company_id=$1 AND task_id=$2",
    )
    .bind(fx.company.id)
    .bind(run.identity.task_id)
    .fetch_all(&fx.pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), run.reservations.len());
    for reservation in &run.reservations {
        let row = rows
            .iter()
            .find(|row| row["request_id"] == reservation.request_id.0.to_string())
            .unwrap();
        assert_eq!(row["agent_id"], run.identity.agent_id.to_string());
        assert_eq!(row["provider"], run.identity.provider.as_str());
        assert_eq!(row["model"], run.identity.model.as_str());
        assert!(row.get("run_id").unwrap().is_null());
        assert_eq!(row["reserved_input"], reservation.input_tokens);
        assert_eq!(row["reserved_output"], reservation.output_tokens);
        if let Some(turn) = run
            .turns
            .iter()
            .find(|turn| turn.request_id == reservation.request_id)
        {
            assert_eq!(row["input_tokens"], turn.input_tokens);
            assert_eq!(row["output_tokens"], turn.output_tokens);
            assert_eq!(
                row["usage_source"],
                serde_json::to_value(turn.token_usage_source).unwrap()
            );
            assert!(!row["settled_at"].is_null());
            if turn.token_usage_source == crate::services::harness::runs::TokenUsageSource::Mixed {
                assert_eq!(turn.output_tokens, reservation.output_tokens);
                assert_eq!(row["usage_source"], "mixed");
            }
        } else {
            assert!(row["settled_at"].is_null());
            assert!(row["usage_source"].is_null());
        }
    }
    assert_report(fx, run).await;
}

async fn assert_report(fx: &Fixture, run: &RunCheckpoint) {
    let actor = sqlx::query_scalar("SELECT user_id FROM companies WHERE id=$1")
        .bind(fx.company.id)
        .fetch_one(&fx.pool)
        .await
        .unwrap();
    let query = UsageQuery {
        scope: UsageScope {
            company: fx.company.id,
            actor,
        },
        start: Utc::now() - chrono::Duration::days(1),
        end: Utc::now() + chrono::Duration::minutes(1),
        grouping: UsageGrouping::Model,
        agent: Some(run.identity.agent_id),
        workflow: None,
    };
    let report = fx.persistence.usage_report(query).await.unwrap();
    assert_eq!(report.groups.len(), 1);
    let group = &report.groups[0];
    assert_eq!(group.provider.as_ref(), Some(&run.identity.provider));
    assert_eq!(group.model.as_ref(), Some(&run.identity.model));
    assert_eq!(group.calls as usize, run.reservations.len());
    assert_eq!(group.completed_calls as usize, run.turns.len());
    assert_eq!(
        group.unconfirmed_calls as usize,
        run.reservations.len() - run.turns.len()
    );
    let usage = crate::services::harness::runs::RunUsage::from(run);
    assert_eq!(group.input_tokens, usage.tokens.prompt_tokens as u64);
    assert_eq!(group.output_tokens, usage.tokens.completion_tokens as u64);
    assert_eq!(
        group.mixed_calls as usize,
        run.turns
            .iter()
            .filter(|turn| turn.token_usage_source
                == crate::services::harness::runs::TokenUsageSource::Mixed)
            .count()
    );
}
