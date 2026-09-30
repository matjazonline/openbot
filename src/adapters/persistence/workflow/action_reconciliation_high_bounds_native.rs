use super::*;

#[derive(Clone, Copy)]
pub(super) enum Attack {
    Entry,
    Consumption,
    HistoricalSpoof,
    HistoricalRetrofit,
}

#[derive(Debug, PartialEq, sqlx::Type)]
#[sqlx(transparent)]
struct ReservationIdentity(String);

type PgTx<'a> = sqlx::Transaction<'a, sqlx::Postgres>;

async fn xid(tx: &mut PgTx<'_>) -> ReservationIdentity {
    sqlx::query_scalar("SELECT pg_current_xact_id()::text")
        .fetch_one(&mut **tx)
        .await
        .unwrap()
}

async fn insert_entry(
    tx: &mut PgTx<'_>,
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
) -> Uuid {
    let entry = Uuid::new_v4();
    let marker = scoped_marker(f, request).await;
    assert_eq!(sqlx::query("INSERT INTO workflow_action_remote_entries(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,id,job_id,attempt_number,execution_generation,worker_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)")
        .bind(request.scope().company.as_uuid()).bind(request.scope().run.as_uuid())
        .bind(request.scope().execution.as_uuid()).bind(request.subject.invocation.as_uuid())
        .bind(request.subject.argument_digest.as_str()).bind(marker).bind(entry)
        .bind(request.fence.scope.job.0).bind(request.fence.attempt.0)
        .bind(request.fence.generation.0).bind(request.fence.worker.0)
        .execute(&mut **tx).await.unwrap().rows_affected(), 1);
    entry
}

pub(super) async fn historical_entry(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
) -> Uuid {
    assert_subject(f, request, 129, "processing").await;
    let before = all_tables(f).await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    let current = xid(&mut tx).await;
    let entry = insert_entry(&mut tx, f, request).await;
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let actual: ReservationIdentity = sqlx::query_scalar("SELECT reservation_xid::text FROM workflow_action_remote_entries WHERE company_id=$1 AND id=$2")
        .bind(request.scope().company.as_uuid()).bind(entry).fetch_one(f.persistence().pool()).await.unwrap();
    assert_eq!(
        actual, current,
        "actual COMMIT retains native source identity"
    );
    reservation_delta(&before, &all_tables(f).await);
    eprintln!(
        "high_bounds historical129 actual_commit entries_delta=1 consumption_delta=0 xid={}",
        actual.0
    );
    entry
}

fn assert_native(error: &sqlx::Error, attack: Attack) {
    let native = error
        .as_database_error()
        .unwrap()
        .downcast_ref::<sqlx::postgres::PgDatabaseError>();
    struct Diagnostic {
        code: &'static str,
        message: &'static str,
        function: &'static str,
    }
    let Diagnostic {
        code,
        message,
        function,
    } = match attack {
        Attack::Entry => Diagnostic {
            code: "P0001",
            message: "workflow action remote entry lacks supported replay",
            function: "workflow_action_remote_entry_commit_guard()",
        },
        Attack::Consumption | Attack::HistoricalRetrofit => Diagnostic {
            code: "23514",
            message: "workflow action final proof cannot be consumed",
            function: "workflow_action_consumption_commit_guard()",
        },
        Attack::HistoricalSpoof => Diagnostic {
            code: "23514",
            message: "invalid workflow action reservation transaction",
            function: "workflow_action_reservation_identity()",
        },
    };
    assert_eq!(native.code(), code);
    assert_eq!(native.message(), message);
    assert!(native.schema().is_none());
    assert!(native.constraint().is_none());
    assert!(native.table().is_none());
    assert!(native.r#where().unwrap().contains(function));
    eprintln!("high_bounds native function={function} code={code} message={message}");
}

async fn flush(tx: &mut PgTx<'_>, attack: Attack) -> sqlx::Error {
    let statement = match attack {
        Attack::Entry => {
            "SET CONSTRAINTS public.workflow_action_remote_entry_commit_guard IMMEDIATE"
        }
        Attack::Consumption | Attack::HistoricalRetrofit => {
            "SET CONSTRAINTS public.workflow_action_consumption_commit_guard IMMEDIATE"
        }
        Attack::HistoricalSpoof => unreachable!(),
    };
    sqlx::query(statement).execute(&mut **tx).await.unwrap_err()
}

pub(super) async fn invalid_exclusions(source: &HighSource) {
    clean_current(source).await;
    let before = all_tables(&source.fixture).await;
    for excluded in [
        None,
        Some(Uuid::new_v4()),
        Some(source.historical),
        Some(source.other_invocation),
        Some(source.other_company),
    ] {
        assert_eq!(
            available(&source.fixture, &source.request, excluded).await,
            None
        );
    }
    let identities: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_action_remote_entries WHERE company_id=$1 AND invocation_id<>$2 AND id=$3) AND EXISTS(SELECT 1 FROM workflow_action_remote_entries WHERE company_id<>$1 AND id=$4)")
        .bind(source.request.scope().company.as_uuid()).bind(source.request.subject.invocation.as_uuid())
        .bind(source.other_invocation).bind(source.other_company).fetch_one(source.fixture.persistence().pool()).await.unwrap();
    assert!(
        identities,
        "exact real other invocation and same-database foreign-company entries"
    );
    assert_eq!(all_tables(&source.fixture).await, before);
}

async fn insert_consumption(tx: &mut PgTx<'_>, source: &HighSource, entry: Uuid) {
    assert_eq!(sqlx::query("INSERT INTO workflow_action_evidence_consumptions(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,evidence_id,remote_entry_id) SELECT company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,$3,id FROM workflow_action_remote_entries WHERE company_id=$1 AND id=$2")
        .bind(source.request.scope().company.as_uuid()).bind(entry).bind(source.proof)
        .execute(&mut **tx).await.unwrap().rows_affected(), 1);
}

async fn candidate_predicates(
    tx: &mut PgTx<'_>,
    source: &HighSource,
    entry: Uuid,
    current: &ReservationIdentity,
) {
    let request = &source.request;
    let valid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_action_evidence_consumptions AS consumption JOIN workflow_action_remote_entries AS entry ON (entry.company_id,entry.run_id,entry.execution_id,entry.invocation_id,entry.argument_digest,entry.dispatch_id,entry.id)=(consumption.company_id,consumption.run_id,consumption.execution_id,consumption.invocation_id,consumption.argument_digest,consumption.dispatch_id,consumption.remote_entry_id) JOIN workflow_action_evidence AS evidence ON (evidence.company_id,evidence.run_id,evidence.execution_id,evidence.invocation_id,evidence.argument_digest,evidence.dispatch_id,evidence.id)=(consumption.company_id,consumption.run_id,consumption.execution_id,consumption.invocation_id,consumption.argument_digest,consumption.dispatch_id,consumption.evidence_id) WHERE consumption.company_id=$1 AND consumption.evidence_id=$2 AND consumption.remote_entry_id=$3 AND entry.reservation_xid=consumption.reservation_xid AND entry.reservation_xid::text=$4 AND evidence.grant_eligible AND evidence.valid_until>clock_timestamp() AND entry.attempt_number=130 AND entry.job_id=$5 AND entry.execution_generation=$6 AND entry.worker_id=$7) AND (SELECT count(*) FROM workflow_action_evidence_consumptions WHERE company_id=$1 AND evidence_id=$2)=1")
        .bind(request.scope().company.as_uuid()).bind(source.proof).bind(entry).bind(&current.0)
        .bind(request.fence.scope.job.0).bind(request.fence.generation.0).bind(request.fence.worker.0)
        .fetch_one(&mut **tx).await.unwrap();
    assert!(
        valid,
        "complete matching immutable scope, current xid, unique genuine consumption"
    );
    let prior: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_action_remote_entries WHERE company_id=$1 AND invocation_id=$2 AND id<>$3")
        .bind(request.scope().company.as_uuid()).bind(request.subject.invocation.as_uuid()).bind(entry)
        .fetch_one(&mut **tx).await.unwrap();
    assert_eq!(prior, 129);
    let available: Option<Uuid> =
        sqlx::query_scalar("SELECT workflow_action_not_applied_available($1,$2,$3)")
            .bind(request.scope().company.as_uuid())
            .bind(request.subject.invocation.as_uuid())
            .bind(entry)
            .fetch_one(&mut **tx)
            .await
            .unwrap();
    assert_eq!(available, None);
    eprintln!(
        "high_bounds candidate130 current_identity=matching prior_count=129 coverage=128 uncovered129=true shared_failures=count+coverage"
    );
}

pub(super) async fn reject_candidate(source: &HighSource, attack: Attack) {
    clean_current(source).await;
    let mut tx = source.fixture.persistence().pool().begin().await.unwrap();
    let current = xid(&mut tx).await;
    let entry = insert_entry(&mut tx, &source.fixture, &source.request).await;
    insert_consumption(&mut tx, source, entry).await;
    candidate_predicates(&mut tx, source, entry, &current).await;
    let error = flush(&mut tx, attack).await;
    assert_native(&error, attack);
    tx.rollback().await.unwrap();
}

pub(super) async fn reject_historical(source: &HighSource, attack: Attack) {
    clean_current(source).await;
    let mut tx = source.fixture.persistence().pool().begin().await.unwrap();
    let current = xid(&mut tx).await;
    let prior: ReservationIdentity = sqlx::query_scalar("SELECT reservation_xid::text FROM workflow_action_remote_entries WHERE company_id=$1 AND id=$2")
        .bind(source.request.scope().company.as_uuid()).bind(source.historical)
        .fetch_one(&mut *tx).await.unwrap();
    assert_ne!(current, prior);
    let error = match attack {
        Attack::HistoricalSpoof => sqlx::query("INSERT INTO workflow_action_evidence_consumptions(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,evidence_id,remote_entry_id,reservation_xid) SELECT company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,$3,id,$4::text::xid8 FROM workflow_action_remote_entries WHERE company_id=$1 AND id=$2")
            .bind(source.request.scope().company.as_uuid()).bind(source.historical).bind(source.proof).bind(&prior.0)
            .execute(&mut *tx).await.unwrap_err(),
        Attack::HistoricalRetrofit => {
            insert_consumption(&mut tx, source, source.historical).await;
            let actual: ReservationIdentity = sqlx::query_scalar("SELECT reservation_xid::text FROM workflow_action_evidence_consumptions WHERE company_id=$1 AND evidence_id=$2")
                .bind(source.request.scope().company.as_uuid()).bind(source.proof).fetch_one(&mut *tx).await.unwrap();
            assert_eq!(actual, current);
            let available: Option<Uuid> = sqlx::query_scalar("SELECT workflow_action_not_applied_available($1,$2,$3)")
                .bind(source.request.scope().company.as_uuid()).bind(source.request.subject.invocation.as_uuid())
                .bind(source.historical).fetch_one(&mut *tx).await.unwrap();
            assert_eq!(available, Some(source.proof), "excluding uncovered129 leaves genuine128 coverage; identity is attacked");
            eprintln!("high_bounds historical_retrofit historical_xid_differs=true excluded_prior_count=128 helper=available");
            flush(&mut tx, attack).await
        }
        _ => unreachable!(),
    };
    assert_native(&error, attack);
    tx.rollback().await.unwrap();
}

pub(super) async fn supported_positive(source: &HighSource) {
    clean_current(source).await;
    let f = &source.fixture;
    assert_eq!(available(f, &source.request, None).await, None);
    let before = all_tables(f).await;
    let writer = adapter(f, 0);
    let RemoteReservationResult::Reserved(reservation) = writer
        .reserve_remote(&source.request, Some(&source.provider.contract))
        .await
        .unwrap()
    else {
        panic!("genuine supported replay remains available above the proof bound");
    };
    let committed = all_tables(f).await;
    reservation_delta(&before, &committed);
    let request = &source.request;
    let provenance: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_action_remote_entries AS entry JOIN workflow_action_dispatches AS marker ON (marker.company_id,marker.run_id,marker.execution_id,marker.invocation_id,marker.argument_digest,marker.id)=(entry.company_id,entry.run_id,entry.execution_id,entry.invocation_id,entry.argument_digest,entry.dispatch_id) JOIN workflow_action_remote_entries AS historical ON historical.company_id=entry.company_id AND historical.id=$8 WHERE entry.company_id=$1 AND entry.run_id=$2 AND entry.execution_id=$3 AND entry.invocation_id=$4 AND entry.id=$5 AND entry.attempt_number=130 AND entry.execution_generation=$6 AND entry.worker_id=$7 AND entry.reservation_xid IS NOT NULL AND entry.reservation_xid<>historical.reservation_xid)")
        .bind(request.scope().company.as_uuid()).bind(request.scope().run.as_uuid())
        .bind(request.scope().execution.as_uuid()).bind(request.subject.invocation.as_uuid())
        .bind(reservation.entry).bind(request.fence.generation.0).bind(request.fence.worker.0)
        .bind(source.historical).fetch_one(f.persistence().pool()).await.unwrap();
    assert!(
        provenance,
        "native committed entry130 has the exact marker/fence and its fresh transaction identity"
    );
    assert_eq!(
        subject_counts(f, &source.request).await,
        EntryAccounting {
            entries: 130,
            consumptions: 0,
            requests: 128
        }
    );
    writer.enter_remote(*reservation).await.unwrap();
    assert_eq!(all_tables(f).await, committed);
    assert_eq!(source.provider.inner.calls.load(Ordering::SeqCst), 128);
    assert_eq!(effects(f).await, 0);
    eprintln!(
        "high_bounds supported_positive committed_entries=130 consumptions=0 calls=128 effects=0"
    );
}
