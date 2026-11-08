//! Historical count fixtures, not a simulated end-to-end producer journey.
//! Only in ManualHistory's OwnDatabase. Actual limits, FKs and CHECKs remain unchanged.
//! Source jobs/executions are copied from a genuine cancelled pre-model call. USER lifecycle
//! triggers are disabled transactionally for that old prefix, then restored before real competitors.
use super::*;
use crate::adapters::persistence::workflow::agent_history_retention_fixture as clock;

pub(super) enum PayloadPrefix {
    Retained,
    Expired,
}
impl PayloadPrefix {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Retained => "retained",
            Self::Expired => "expired",
        }
    }
}
pub(super) struct Prefix {
    pub scope: crate::application::workflow::activation::ActivationRequest,
    pub conversation: Uuid,
}

pub(super) async fn seed(f: &mut ManualHistory, count: u16, mode: PayloadPrefix) -> Prefix {
    assert!((1..=4096).contains(&count));
    f.from_run = None;
    let claim = f.start(json!("historical count fixture")).await;
    f.f.s
        .p()
        .prepare_agent(&claim, policy())
        .await
        .unwrap()
        .expect_ready();
    cancel(
        &f.f,
        &claim,
        &format!("prefix-source-{}", claim.fence.scope.run.as_uuid()),
    )
    .await;
    let scope = claim.fence.scope;
    let prepared: Value = sqlx::query_scalar(
        "SELECT frozen_preparation FROM agent_history_exchanges WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.execution.as_uuid())
    .fetch_one(f.f.s.pool())
    .await
    .unwrap();
    let conversation: Uuid = sqlx::query_scalar(
        "SELECT conversation_id FROM agent_history_exchanges WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.execution.as_uuid())
    .fetch_one(f.f.s.pool())
    .await
    .unwrap();
    if matches!(mode, PayloadPrefix::Expired) {
        clock::expire(f.f.s.p(), scope).await;
    } else {
        clock::close_retry_window(f.f.s.p(), scope).await;
    }
    let snapshot = SeedSnapshot {
        scope,
        conversation,
        prepared,
    };
    populate(f, &snapshot, count, mode).await;
    Prefix {
        scope,
        conversation,
    }
}

struct SeedSnapshot {
    scope: crate::application::workflow::activation::ActivationRequest,
    conversation: Uuid,
    prepared: Value,
}

async fn populate(f: &ManualHistory, snapshot: &SeedSnapshot, count: u16, mode: PayloadPrefix) {
    let scope = snapshot.scope;
    let conversation = snapshot.conversation;
    let prepared = &snapshot.prepared;
    let mut tx = f.f.s.pool().begin().await.unwrap();
    for table in [
        "workflow_executions",
        "background_tasks",
        "agent_history_exchanges",
        "agent_history_conversations",
    ] {
        sqlx::query(&format!("ALTER TABLE {table} DISABLE TRIGGER USER"))
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    // SQLx's extended protocol accepts one statement per query; bind the identical native source
    // identities to each count-fixture statement, never string-interpolate payloads or identifiers.
    for statement in include_str!("agent_history_capacity_prefix.sql")
        .split(';')
        .filter(|sql| !sql.trim().is_empty())
    {
        let mut query = sqlx::query(statement);
        let maximum = (1..=8)
            .rev()
            .find(|number| statement.contains(&format!("${number}")))
            .unwrap_or(0);
        for number in 1..=maximum {
            query = match number {
                1 => query.bind(scope.company.as_uuid()),
                2 => query.bind(scope.run.as_uuid()),
                3 => query.bind(scope.execution.as_uuid()),
                4 => query.bind(scope.job.0),
                5 => query.bind(conversation),
                6 => query.bind(i64::from(count)),
                7 => query.bind(prepared),
                8 => query.bind(mode.as_str()),
                _ => unreachable!(),
            };
        }
        query.execute(&mut *tx).await.unwrap();
    }
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(&mut *tx)
        .await
        .unwrap();
    for table in [
        "workflow_executions",
        "background_tasks",
        "agent_history_exchanges",
        "agent_history_conversations",
    ] {
        sqlx::query(&format!("ALTER TABLE {table} ENABLE TRIGGER USER"))
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    sqlx::query("ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let guarded:bool=sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid=ANY(ARRAY['workflow_executions'::regclass,'background_tasks'::regclass,'agent_history_exchanges'::regclass,'agent_history_conversations'::regclass]) AND tgenabled<>'O')")
        .fetch_one(f.f.s.pool()).await.unwrap();
    assert!(
        guarded,
        "every native guard is restored before exercising real producers"
    );
}
