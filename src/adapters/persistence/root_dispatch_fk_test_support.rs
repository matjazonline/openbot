//! Probe the real frozen local-call row, restoring it exactly after every transaction.
use super::{FkExpectation, PostgresPersistence, finish_fk_probe};
use crate::{
    application::workflow::actions::ActionScope,
    domain::workflow::{CompanyId, ExecutionId, RunId},
};
use serde_json::{Value, json};
use uuid::Uuid;

const DISPATCH: &str = "workflow_local_agent_calls_company_id_run_id_execution_id_fkey2";
const OWNER: &str = "workflow_local_agent_calls_company_id_run_id_execution_id__fkey";
const PROFILE: &str = "workflow_local_agent_calls_company_id_profile_id_fkey";
const OPERATION: &str = "workflow_local_agent_calls_company_id_machine_id_installat_fkey";
const MACHINE_SESSION: &str = "workflow_local_agent_calls_company_id_machine_id_installa_fkey1";
const PROFILE_SESSION: &str = "workflow_local_agent_calls_company_id_profile_id_session_i_fkey";
const COLUMNS: &str = "company_id,run_id,execution_id,invocation_id,argument_digest,profile_id,machine_id,installation_id,session_id,operation_id,operation_origin,wait_id,dispatch_id,fence_expires_at,effect_kind,ready_at,workflow_outcome,created_at";

#[derive(sqlx::FromRow)]
struct ForeignPeers {
    profile_id: Uuid,
    machine_id: Uuid,
    installation_id: Uuid,
    session_id: Uuid,
}

struct PeerProbe {
    constraint: &'static str,
    field: &'static str,
    foreign_id: Uuid,
}

fn id(row: &Value, field: &str) -> Uuid {
    serde_json::from_value(row[field].clone()).unwrap()
}

async fn source(p: &PostgresPersistence, scope: ActionScope) -> Value {
    sqlx::query_scalar("SELECT to_jsonb(call) FROM workflow_local_agent_calls AS call WHERE company_id=$1 AND run_id=$2 AND execution_id=$3")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
        .fetch_one(p.pool()).await.unwrap()
}

async fn replace(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    scope: ActionScope,
    candidate: Value,
) -> Result<(), sqlx::Error> {
    let deleted = sqlx::query("DELETE FROM workflow_local_agent_calls WHERE company_id=$1 AND run_id=$2 AND execution_id=$3")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
        .execute(&mut **tx).await?;
    assert_eq!(deleted.rows_affected(), 1);
    let statement = format!(
        "INSERT INTO workflow_local_agent_calls({COLUMNS}) SELECT {COLUMNS} FROM jsonb_populate_record(NULL::public.workflow_local_agent_calls,$1) AS cloned"
    );
    sqlx::query(&statement)
        .bind(candidate)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn candidate_probe(
    p: &PostgresPersistence,
    scope: ActionScope,
    original: &Value,
    candidate: Value,
    expected: FkExpectation,
) {
    let mut tx = p.pool().begin().await.unwrap();
    replace(&mut tx, scope, candidate).await.unwrap();
    finish_fk_probe(tx, expected).await;
    assert_eq!(source(p, scope).await, *original);
}

async fn foreign_owner_probe(
    p: &PostgresPersistence,
    scope: ActionScope,
    original: &Value,
    foreign: CompanyId,
) {
    let mut candidate = original.clone();
    candidate["company_id"] = json!(foreign.as_uuid());
    let mut tx = p.pool().begin().await.unwrap();
    let error = replace(&mut tx, scope, candidate).await.unwrap_err();
    let database = error.as_database_error().unwrap();
    let constraint = database.constraint().unwrap();
    match constraint {
        OWNER => assert_eq!(database.code().as_deref(), Some("23503")),
        "workflow_guard:local_agent_wake" => assert_eq!(database.code().as_deref(), Some("23514")),
        _ => panic!("foreign source must fail its exact owner/frozen-source guard: {error}"),
    }
    eprintln!(
        "Foreign local-call source refused by {constraint}; the dispatch peer was not isolated by this probe"
    );
    tx.rollback().await.unwrap();
    assert_eq!(source(p, scope).await, *original);
}

fn changed(original: &Value, fields: &[(&str, Uuid)]) -> Value {
    let mut candidate = original.clone();
    for (field, value) in fields {
        candidate[*field] = json!(value);
    }
    candidate
}

async fn peer_edges(
    p: &PostgresPersistence,
    scope: ActionScope,
    original: &Value,
    foreign: CompanyId,
) {
    let foreign: ForeignPeers = sqlx::query_as("SELECT profile_id,machine_id,installation_id,id AS session_id FROM local_agent_sessions WHERE company_id=$1 ORDER BY id LIMIT 1")
        .bind(foreign.as_uuid()).fetch_one(p.pool()).await.unwrap();
    for peer in [
        PeerProbe {
            constraint: PROFILE,
            field: "profile_id",
            foreign_id: foreign.profile_id,
        },
        PeerProbe {
            constraint: MACHINE_SESSION,
            field: "session_id",
            foreign_id: foreign.session_id,
        },
        PeerProbe {
            constraint: PROFILE_SESSION,
            field: "session_id",
            foreign_id: foreign.session_id,
        },
    ] {
        for value in [Uuid::new_v4(), peer.foreign_id] {
            candidate_probe(
                p,
                scope,
                original,
                changed(original, &[(peer.field, value)]),
                FkExpectation::Refused(peer.constraint),
            )
            .await;
        }
    }
    // Invocation identity prevents substituting a foreign operation id. Exercise
    // the exact operation FK with missing and actual foreign native machine tuples.
    for fields in [
        [
            ("machine_id", Uuid::new_v4()),
            ("installation_id", Uuid::new_v4()),
        ],
        [
            ("machine_id", foreign.machine_id),
            ("installation_id", foreign.installation_id),
        ],
    ] {
        candidate_probe(
            p,
            scope,
            original,
            changed(original, &fields),
            FkExpectation::Refused(OPERATION),
        )
        .await;
    }
}

async fn immutable_guards(p: &PostgresPersistence, scope: ActionScope, original: &Value) {
    let refused = sqlx::query("UPDATE workflow_local_agent_calls SET dispatch_id=$4 WHERE company_id=$1 AND run_id=$2 AND execution_id=$3")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
        .bind(Uuid::new_v4()).execute(p.pool()).await.unwrap_err();
    assert_eq!(
        refused.as_database_error().unwrap().constraint(),
        Some("workflow_guard:local_agent_identity")
    );
    let refused =
        sqlx::query("DELETE FROM workflow_action_dispatches WHERE company_id=$1 AND id=$2")
            .bind(scope.company.as_uuid())
            .bind(id(original, "dispatch_id"))
            .execute(p.pool())
            .await
            .unwrap_err();
    assert_eq!(
        refused.as_database_error().unwrap().constraint(),
        Some("workflow_guard:workflow_action_fact_immutable")
    );
    assert_eq!(source(p, scope).await, *original);
}

pub(crate) async fn assert_relationship(
    p: &PostgresPersistence,
    company: CompanyId,
    foreign: CompanyId,
) {
    let original: Value = sqlx::query_scalar("SELECT to_jsonb(call) FROM workflow_local_agent_calls AS call WHERE company_id=$1 AND dispatch_id IS NOT NULL ORDER BY run_id,execution_id LIMIT 1")
        .bind(company.as_uuid()).fetch_one(p.pool()).await.unwrap();
    let scope = ActionScope {
        company,
        run: RunId::new(id(&original, "run_id")),
        execution: ExecutionId::new(id(&original, "execution_id")),
    };
    let marker = id(&original, "dispatch_id");
    let wrong_marker: Uuid = sqlx::query_scalar("SELECT id FROM workflow_action_dispatches WHERE company_id=$1 AND id<>$2 ORDER BY id LIMIT 1")
        .bind(company.as_uuid()).bind(marker).fetch_one(p.pool()).await.unwrap();
    candidate_probe(p, scope, &original, original.clone(), FkExpectation::Valid).await;
    candidate_probe(
        p,
        scope,
        &original,
        changed(&original, &[("dispatch_id", Uuid::new_v4())]),
        FkExpectation::Refused(DISPATCH),
    )
    .await;
    candidate_probe(
        p,
        scope,
        &original,
        changed(&original, &[("dispatch_id", wrong_marker)]),
        FkExpectation::Refused(DISPATCH),
    )
    .await;
    peer_edges(p, scope, &original, foreign).await;
    foreign_owner_probe(p, scope, &original, foreign).await;
    immutable_guards(p, scope, &original).await;
}
