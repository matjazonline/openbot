use super::*;
use crate::application::workflow::storage::*;

#[path = "autonomous_agent_origin_fixture.rs"]
mod support;

#[derive(Debug)]
enum NativeFailure {
    Manifest,
    Selection,
    Binding,
    Shape,
    Content,
}
impl NativeFailure {
    fn constraint(&self) -> &'static str {
        match self {
            Self::Manifest => "workflow_action_origin_manifest_fk",
            Self::Selection => "workflow_action_origin_selection_fk",
            Self::Binding => "workflow_action_origin_binding_fk",
            Self::Shape => "workflow_action_origin_shape",
            Self::Content => "workflow_action_origin_content",
        }
    }
    fn code(&self) -> &'static str {
        match self {
            Self::Shape | Self::Content => "23514",
            _ => "23503",
        }
    }
}

fn native_row(original: &Value, patch: Value) -> Value {
    let mut row = original.clone();
    let id = Uuid::new_v4();
    let digest = id.simple().to_string().repeat(2);
    row["id"] = json!(id);
    row["operation_digest"] = json!(digest);
    row["idempotency_key"] = json!(format!("workflow-action:v1:{digest}"));
    row.as_object_mut()
        .unwrap()
        .extend(patch.as_object().unwrap().clone());
    let origin = if row["origin_kind"] == "workflow" {
        json!({"kind":"workflow"})
    } else {
        json!({"kind":"agent","manifest":row["agent_manifest_id"],"agent":row["agent_id"],"revision":row["agent_revision_id"],"binding":row["agent_binding_id"]})
    };
    row["operation"]["context"]["origin"] = origin;
    row
}

async fn native_check(p: &PostgresPersistence, row: Value, expected: Option<NativeFailure>) {
    let mut tx = p.pool().begin().await.unwrap();
    // Deliberately bypass application guards; reseeded digest/key values satisfy
    // the unrelated uniqueness/format constraints. Every candidate is rolled back.
    let inserted=sqlx::query("INSERT INTO workflow_action_intents SELECT (jsonb_populate_record(NULL::workflow_action_intents,$1)).*")
        .bind(row).execute(&mut *tx).await;
    let result = match inserted {
        Ok(_) => {
            sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
                .execute(&mut *tx)
                .await
        }
        Err(error) => Err(error),
    };
    if let Some(expected) = expected {
        let error = result.unwrap_err();
        let error = error.as_database_error().expect("native constraint error");
        assert_eq!(
            error.code().as_deref(),
            Some(expected.code()),
            "{expected:?}: {error}"
        );
        assert_eq!(error.constraint(), Some(expected.constraint()), "{error}");
    } else {
        result.unwrap();
    }
    tx.rollback().await.unwrap();
}

#[derive(Debug, PartialEq, sqlx::FromRow)]
struct ActionHistory {
    intents: i64,
    dispatches: i64,
    receipts: i64,
}

async fn history(p: &PostgresPersistence, company: CompanyId, run: RunId) -> ActionHistory {
    sqlx::query_as("SELECT
        (SELECT count(*) FROM workflow_action_intents WHERE company_id=$1 AND run_id=$2) AS intents,
        (SELECT count(*) FROM workflow_action_dispatches WHERE company_id=$1 AND run_id=$2) AS dispatches,
        (SELECT count(*) FROM workflow_action_receipts WHERE company_id=$1 AND run_id=$2) AS receipts")
        .bind(company.as_uuid()).bind(run.as_uuid()).fetch_one(p.pool()).await.unwrap()
}

struct OriginCase {
    patch: Value,
    failure: NativeFailure,
}

#[tokio::test]
async fn autonomous_agent_storage_action_origin_native_scope_selection_binding_and_content() {
    let f = support::populated().await;
    let p = &f.setup.fixture.persistence;
    let company = f.setup.fixture.target.company;
    let run = f.setup.claim.fence.scope.run;
    let before = history(p, company, run).await;
    assert_eq!(
        before,
        ActionHistory {
            intents: 1,
            dispatches: 1,
            receipts: 1
        }
    );
    native_check(p, native_row(&f.original, json!({})), None).await;
    let survivor_manifest: Uuid = sqlx::query_scalar(
        "SELECT manifest_id FROM run_dependency_manifests WHERE company_id=$1 AND run_id=$2",
    )
    .bind(company.as_uuid())
    .bind(f.survivor.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    let additional = f
        .later
        .definition
        .tools
        .iter()
        .find(|tool| tool.binding.alias.as_str() == "additional.request")
        .unwrap()
        .binding
        .id;
    let cases = [
        OriginCase {
            patch: json!({"agent_manifest_id":survivor_manifest}),
            failure: NativeFailure::Manifest,
        },
        OriginCase {
            patch: json!({"agent_revision_id":f.later.id}),
            failure: NativeFailure::Selection,
        },
        OriginCase {
            patch: json!({"agent_binding_id":additional}),
            failure: NativeFailure::Binding,
        },
        OriginCase {
            patch: json!({"agent_id":f.foreign.definition.agent_id}),
            failure: NativeFailure::Selection,
        },
        OriginCase {
            patch: json!({"origin_kind":"workflow"}),
            failure: NativeFailure::Shape,
        },
    ];
    for case in cases {
        native_check(p, native_row(&f.original, case.patch), Some(case.failure)).await;
    }
    let mut disagreement = native_row(&f.original, json!({}));
    disagreement["operation"]["context"]["origin"]["binding"] = json!(Uuid::new_v4());
    native_check(p, disagreement, Some(NativeFailure::Content)).await;
    assert_eq!(before, history(p, company, run).await);
}

async fn old_root(f: &support::OriginHistory) -> RetentionRoot {
    let p = &f.setup.fixture.persistence;
    let scope = f.setup.claim.fence.scope;
    let current = p.head(scope.company, scope.run).await.unwrap().unwrap();
    let result = p
        .cancel(CancelCommand {
            company_id: scope.company,
            run_id: scope.run,
            actor: f.setup.fixture.target.actor,
            command_key: IdempotencyKey::parse("origin-history-retire").unwrap(),
            expected_revision: current.revision,
        })
        .await
        .unwrap();
    assert!(matches!(result, CancelResult::Applied { .. }));
    let finished_at=sqlx::query_scalar("UPDATE workflow_runs SET finished_at=clock_timestamp()-interval '91 days' WHERE company_id=$1 AND id=$2 RETURNING finished_at")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    RetentionRoot {
        company: scope.company,
        run: scope.run,
        finished_at,
    }
}

#[tokio::test]
async fn autonomous_agent_storage_action_origin_populated_collector_and_company_cascade() {
    let f = support::populated().await;
    let p = &f.setup.fixture.persistence;
    let company = f.setup.fixture.target.company;
    let run = f.setup.claim.fence.scope.run;
    let populated = ActionHistory {
        intents: 1,
        dispatches: 1,
        receipts: 1,
    };
    let empty = ActionHistory {
        intents: 0,
        dispatches: 0,
        receipts: 0,
    };
    assert_eq!(history(p, company, run).await, populated);
    assert_eq!(history(p, company, f.survivor).await, populated);
    let root = old_root(&f).await;
    let (left, right) = tokio::join!(p.retire_tree(root), p.retire_tree(root));
    assert_eq!(
        usize::from(left.unwrap().is_some()) + usize::from(right.unwrap().is_some()),
        1
    );
    assert_eq!(history(p, company, run).await, empty);
    assert_eq!(history(p, company, f.survivor).await, populated);
    let retained:Uuid=sqlx::query_scalar("SELECT member.revision_id FROM run_dependency_manifests AS owner JOIN agent_manifest_revisions AS member ON member.company_id=owner.company_id AND member.manifest_id=owner.manifest_id WHERE owner.company_id=$1 AND owner.run_id=$2 AND member.agent_id=$3")
        .bind(company.as_uuid()).bind(f.survivor.as_uuid()).bind(f.setup.agent).fetch_one(p.pool()).await.unwrap();
    assert_eq!(json!(retained), f.original["agent_revision_id"]);
    let gone: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM run_dependency_manifests WHERE company_id=$1 AND run_id=$2",
    )
    .bind(company.as_uuid())
    .bind(run.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(gone, 0);
    sqlx::query("DELETE FROM companies WHERE id=$1")
        .bind(company.as_uuid())
        .execute(p.pool())
        .await
        .unwrap();
    assert_eq!(history(p, company, f.survivor).await, empty);
    let revisions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM agent_revisions WHERE company_id=$1")
            .bind(company.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(revisions, 0);
    let foreign: i64 =
        sqlx::query_scalar("SELECT count(*) FROM agent_revisions WHERE company_id=$1 AND id=$2")
            .bind(f.foreign.definition.company_id.as_uuid())
            .bind(f.foreign.id.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(
        foreign, 1,
        "the unrelated tenant's retained revision must survive"
    );
}
