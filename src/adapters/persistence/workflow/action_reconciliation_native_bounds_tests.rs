//! PostgreSQL JSON-text limits, independently of canonical Rust issuance limits.
use super::*;

#[path = "action_reconciliation_native_evidence_tests.rs"]
mod native_evidence_tests;

#[derive(Clone, Copy)]
struct RelationName(&'static str);
#[derive(Clone, Copy)]
struct ConstraintName(&'static str);
#[derive(Clone, Copy)]
struct CheckOwner {
    relation: RelationName,
    constraint: ConstraintName,
}

const COMMAND: CheckOwner = CheckOwner {
    relation: RelationName("workflow_action_evidence_commands"),
    constraint: ConstraintName("workflow_action_evidence_commands_outcome_check"),
};
const RECEIPT: CheckOwner = CheckOwner {
    relation: RelationName("workflow_action_receipts"),
    constraint: ConstraintName("workflow_action_receipts_result_check"),
};
const OBSERVATION: CheckOwner = CheckOwner {
    relation: RelationName("workflow_action_actual_receipt_observations"),
    constraint: ConstraintName("workflow_action_actual_receipt_observations_result_check"),
};

async fn native_catalog(f: &AdmissionFixture) -> Value {
    sqlx::query_scalar(
        "SELECT jsonb_build_object( \
          'checks',(SELECT jsonb_agg(jsonb_build_object( \
            'table',relation.relname,'name',constraint_row.conname, \
            'validated',constraint_row.convalidated,'definition',pg_get_constraintdef(constraint_row.oid)) \
            ORDER BY relation.relname,constraint_row.conname) \
            FROM pg_constraint AS constraint_row \
            JOIN pg_class AS relation ON relation.oid=constraint_row.conrelid \
            JOIN pg_namespace AS namespace ON namespace.oid=relation.relnamespace \
            WHERE namespace.nspname='public' AND constraint_row.contype='c'), \
          'triggers',(SELECT jsonb_agg(jsonb_build_object( \
            'table',relation.relname,'name',trigger_row.tgname,'enabled',trigger_row.tgenabled, \
            'definition',pg_get_triggerdef(trigger_row.oid)) ORDER BY relation.relname,trigger_row.tgname) \
            FROM pg_trigger AS trigger_row \
            JOIN pg_class AS relation ON relation.oid=trigger_row.tgrelid \
            JOIN pg_namespace AS namespace ON namespace.oid=relation.relnamespace \
            WHERE namespace.nspname='public' AND NOT trigger_row.tgisinternal), \
          'functions',(SELECT jsonb_agg(jsonb_build_object( \
            'name',procedure.proname,'definition',pg_get_functiondef(procedure.oid)) ORDER BY procedure.oid) \
            FROM pg_proc AS procedure \
            JOIN pg_namespace AS namespace ON namespace.oid=procedure.pronamespace \
            WHERE namespace.nspname='public' AND procedure.prokind IN ('f','p')))",
    )
    .fetch_one(f.persistence().pool())
    .await
    .unwrap()
}

fn check_owner(catalog: &Value, owner: CheckOwner, limit: usize) {
    let check = catalog["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["table"] == owner.relation.0 && row["name"] == owner.constraint.0)
        .expect("exact native CHECK owner exists");
    assert_eq!(check["validated"], json!(true));
    let definition = check["definition"].as_str().unwrap();
    assert!(definition.contains("octet_length"));
    assert!(definition.contains(&format!("<= {limit}")));
    for trigger in catalog["triggers"].as_array().unwrap() {
        assert_eq!(trigger["enabled"], "O", "native trigger stays enabled");
    }
}

fn rejected_check(error: sqlx::Error, owner: CheckOwner) {
    let sqlx::Error::Database(error) = error else {
        panic!("expected native CHECK error, got {error:?}");
    };
    let native = error.downcast_ref::<sqlx::postgres::PgDatabaseError>();
    assert_eq!(native.code(), "23514");
    assert_eq!(native.schema(), Some("public"));
    assert_eq!(native.table(), Some(owner.relation.0));
    assert_eq!(native.constraint(), Some(owner.constraint.0));
    assert_eq!(
        native.message(),
        format!(
            "new row for relation \"{}\" violates check constraint \"{}\"",
            owner.relation.0, owner.constraint.0
        )
    );
}

async fn text_bytes(f: &AdmissionFixture, value: &Value) -> usize {
    let bytes: i32 = sqlx::query_scalar("SELECT octet_length($1::jsonb::text)")
        .bind(value)
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    usize::try_from(bytes).unwrap()
}

async fn padded_object(f: &AdmissionFixture, size: usize) -> Value {
    let base = json!({"padding":""});
    let overhead = text_bytes(f, &base).await;
    let result = json!({"padding":"x".repeat(size - overhead)});
    assert_eq!(text_bytes(f, &result).await, size);
    result
}

#[tokio::test]
async fn workflow_action_reconciliation_native_bounds_command_shape_and_bytes() {
    let (f, request, observed) = Box::pin(prepared()).await;
    let command = verified_command(&f, &request, &observed, "native-command-bounds").await;
    let before = all_tables(&f).await;
    let catalog = native_catalog(&f).await;
    check_owner(&catalog, COMMAND, 1024);
    let exact = padded_object(&f, 1024).await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    assert_eq!(
        insert_outcome_on(&mut tx, &command, &exact)
            .await
            .unwrap()
            .rows_affected(),
        1
    );
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(all_tables(&f).await, before);
    assert_eq!(native_catalog(&f).await, catalog);
    // The bounded object is a SQL structural control, not an application outcome.
    for value in [json!("scalar"), json!([]), padded_object(&f, 1025).await] {
        let mut tx = f.persistence().pool().begin().await.unwrap();
        rejected_check(
            insert_outcome_on(&mut tx, &command, &value)
                .await
                .unwrap_err(),
            COMMAND,
        );
        tx.rollback().await.unwrap();
        assert_eq!(all_tables(&f).await, before);
        assert_eq!(native_catalog(&f).await, catalog);
    }
    assert_eq!(observed.calls.load(Ordering::SeqCst), 0);
}

#[derive(sqlx::FromRow)]
struct ActualResult {
    marker: Uuid,
    entry: Uuid,
    result: Value,
}

struct ResultSource {
    fixture: AdmissionFixture,
    actual: ActualResult,
    provider: LedgerProvider,
}

async fn result_source() -> ResultSource {
    let (f, request) = Box::pin(setup()).await;
    let _registration = ledger(&f, &request).await;
    let result = padded_object(&f, 131_072).await;
    let provider = LedgerProvider::new(
        &f,
        Delivery::Apply {
            result: result.clone(),
            recover: true,
            lose: true,
        },
    );
    assert!(matches!(
        invoke(&f, &request, &provider).await,
        Err(AppError::Timeout(_))
    ));
    park(&f, &request).await;
    let actual: ActualResult = sqlx::query_as(
        "SELECT entry.dispatch_id AS marker,entry.id AS entry,ledger.result \
         FROM workflow_action_remote_entries AS entry \
         JOIN fixture_evidence_ledger AS ledger ON ledger.company_id=entry.company_id \
           AND ledger.invocation_id=entry.invocation_id AND ledger.entry_id=entry.id \
         JOIN fixture_provider_effects AS effect ON effect.entry_id=entry.id AND effect.result=ledger.result \
         WHERE entry.company_id=$1 AND entry.invocation_id=$2",
    )
    .bind(request.scope().company.as_uuid())
    .bind(request.subject.invocation.as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert_eq!(actual.result, result);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(effects(&f).await, 1);
    let before = all_tables(&f).await;
    assert_eq!(before["workflow_action_receipts"], json!([]));
    assert_eq!(
        before["workflow_action_actual_receipt_observations"],
        json!([])
    );
    ResultSource {
        fixture: f,
        actual,
        provider,
    }
}

#[derive(Clone, Copy)]
enum ResultFact {
    Receipt,
    Observation,
}

impl ResultFact {
    fn sql(self) -> &'static str {
        match self {
            Self::Receipt => {
                "INSERT INTO workflow_action_receipts \
                 (company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id, \
                  effect_kind,remote_entry_id,result) \
                 SELECT entry.company_id,entry.run_id,entry.execution_id,entry.invocation_id, \
                  entry.argument_digest,entry.dispatch_id,'remote',entry.id,$3 \
                 FROM workflow_action_remote_entries AS entry WHERE entry.id=$1 AND entry.dispatch_id=$2"
            }
            Self::Observation => {
                "INSERT INTO workflow_action_actual_receipt_observations \
                 (company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,remote_entry_id,result) \
                 SELECT entry.company_id,entry.run_id,entry.execution_id,entry.invocation_id, \
                  entry.argument_digest,entry.dispatch_id,entry.id,$3 \
                 FROM workflow_action_remote_entries AS entry WHERE entry.id=$1 AND entry.dispatch_id=$2"
            }
        }
    }
}

async fn insert_result(
    db: &mut sqlx::PgConnection,
    source: &ActualResult,
    fact: ResultFact,
    result: &Value,
) -> Result<u64, sqlx::Error> {
    sqlx::query(fact.sql())
        .bind(source.entry)
        .bind(source.marker)
        .bind(result)
        .execute(db)
        .await
        .map(|inserted| inserted.rows_affected())
}

enum ExpectedCheck {
    Accepted,
    Rejected(CheckOwner),
}

async fn result_probe(
    source: &ResultSource,
    fact: ResultFact,
    result: &Value,
    expected: ExpectedCheck,
) {
    let f = &source.fixture;
    let before = all_tables(f).await;
    let catalog = native_catalog(f).await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    if matches!(fact, ResultFact::Observation) && matches!(expected, ExpectedCheck::Rejected(_)) {
        assert_eq!(
            insert_result(
                &mut tx,
                &source.actual,
                ResultFact::Receipt,
                &source.actual.result
            )
            .await
            .unwrap(),
            1
        );
        sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    let inserted = insert_result(&mut tx, &source.actual, fact, result).await;
    match expected {
        ExpectedCheck::Rejected(owner) => rejected_check(inserted.unwrap_err(), owner),
        ExpectedCheck::Accepted => {
            assert_eq!(inserted.unwrap(), 1);
            if matches!(fact, ResultFact::Observation) {
                // Insert observation first, then its receipt. Native receipt
                // observation insertion skips this identical generated digest.
                assert_eq!(
                    insert_result(&mut tx, &source.actual, ResultFact::Receipt, result)
                        .await
                        .unwrap(),
                    1
                );
            }
            sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
                .execute(&mut *tx)
                .await
                .unwrap();
            let counts: Value = sqlx::query_scalar(
                "SELECT jsonb_build_object( \
                 'receipts',(SELECT count(*) FROM workflow_action_receipts), \
                 'observations',(SELECT count(*) FROM workflow_action_actual_receipt_observations))",
            )
            .fetch_one(&mut *tx)
            .await
            .unwrap();
            assert_eq!(counts, json!({"receipts":1,"observations":1}));
        }
    }
    tx.rollback().await.unwrap();
    assert_eq!(all_tables(f).await, before);
    assert_eq!(native_catalog(f).await, catalog);
    assert_eq!(source.provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(effects(f).await, 1);
}

#[tokio::test]
async fn workflow_action_reconciliation_native_bounds_actual_results() {
    // This real lost-response source is larger than Rust's validated-result
    // boundary. Only native structural acceptance is claimed here.
    let source = Box::pin(result_source()).await;
    let catalog = native_catalog(&source.fixture).await;
    check_owner(&catalog, RECEIPT, 131_072);
    check_owner(&catalog, OBSERVATION, 131_072);
    let oversized = padded_object(&source.fixture, 131_073).await;
    for (fact, owner) in [
        (ResultFact::Receipt, RECEIPT),
        (ResultFact::Observation, OBSERVATION),
    ] {
        result_probe(
            &source,
            fact,
            &source.actual.result,
            ExpectedCheck::Accepted,
        )
        .await;
        result_probe(&source, fact, &oversized, ExpectedCheck::Rejected(owner)).await;
    }
}
