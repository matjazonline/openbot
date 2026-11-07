use super::*;

#[derive(sqlx::FromRow)]
struct ShortClaim {
    proposal_id: Uuid,
    integrity: String,
    lease_expires_at: chrono::DateTime<chrono::Utc>,
}

async fn unavailable_composer() -> crate::application::transport::DeliveryComposer {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy(&std::env::var("TEST_DATABASE_URL").unwrap())
        .unwrap();
    pool.close().await;
    approval_test_support::composer(&PostgresPersistence::new(pool))
}

#[tokio::test]
async fn channel_reply_recovery_transient_composition_backs_off_and_spares_quiet_tenant() {
    let f = FinalFixture::new("data.map", false).await;
    f.advance().await;
    let intent = f.s.intent(TO).await;
    f.s.drain(&f.s.admissions(admission(&f.p))).await;
    let added: ScopeRow = sqlx::query_as("SELECT execution.run_id,execution.id AS execution_id,job.id AS job_id FROM channel_reply_runs AS owned \
        JOIN workflow_executions AS execution ON execution.company_id=owned.company_id AND execution.run_id=owned.run_id \
        JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id WHERE owned.company_id=$1 AND owned.intent_id=$2")
        .bind(f.scope.company.as_uuid()).bind(intent).fetch_one(f.p.pool()).await.unwrap();
    f.p.advance_pure(
        ActivationRequest {
            run: RunId::new(added.run_id),
            execution: ExecutionId::new(added.execution_id),
            job: WorkflowJobId(added.job_id),
            ..f.scope
        },
        BatchBudget::new(4, Duration::from_millis(100)).unwrap(),
    )
    .await
    .unwrap();
    let unavailable = ChannelReplyPublisher::new(
        f.p.clone(),
        MessagePlanner::new(unavailable_composer().await, "agents.test".into()),
    );
    let before = outbound_count(&f).await;
    let batch =
        f.p.claim_replies(
            ReplyPublicationWorker(Uuid::new_v4()),
            ReplyPublicationPage::try_from(2).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(batch.len(), 2, "a full publication batch fails transiently");
    for claim in batch {
        assert_eq!(
            unavailable.publish_claim(claim).await.unwrap(),
            ReplyPublicationOutcome::Deferred
        );
    }
    assert_eq!(f.state().await["decision"], "pending");
    assert_eq!(outbound_count(&f).await, before);
    assert!(
        f.p.claim_replies(
            ReplyPublicationWorker(Uuid::new_v4()),
            ReplyPublicationPage::try_from(2).unwrap()
        )
        .await
        .unwrap()
        .is_empty(),
        "a poison iteration does not immediately reclaim its row"
    );
    let other = Intents::in_fixture(
        Fixture::in_database(f.s.fixture._db.clone(), "quiet-publication").await,
    )
    .await;
    let quiet = FinalFixture::configured(
        other,
        final_source("data.map"),
        literal_contract("Quiet"),
        false,
    )
    .await;
    quiet.advance().await;
    let current = claim(&quiet).await;
    assert_eq!(current.company, quiet.scope.company);
    assert!(matches!(
        publisher(&quiet).publish_claim(current).await.unwrap(),
        ReplyPublicationOutcome::Published(_)
    ));
}

#[tokio::test]
async fn channel_reply_recovery_reconstructed_worker_polls_without_a_wakeup() {
    let f = FinalFixture::new("data.map", false).await;
    f.advance().await;
    let p = PostgresPersistence::new(f.p.pool().clone());
    let worker = ChannelReplyPublisher::new(
        p.clone(),
        MessagePlanner::new(approval_test_support::composer(&p), "agents.test".into()),
    );
    let shutdown = CancellationToken::new();
    let token = shutdown.clone();
    let owner = tokio::spawn(async move { worker.run(&token).await });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if f.state().await["decision"] == "published" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(4), owner)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(f.state().await["run"], "succeeded");
}

#[tokio::test]
async fn channel_reply_recovery_deferred_commit_expiry_rolls_back_all_effects() {
    let f = FinalFixture::new("data.map", false).await;
    f.advance().await;
    let generation = Uuid::new_v4();
    let worker = ReplyPublicationWorker(Uuid::new_v4());
    let row: ShortClaim = sqlx::query_as("UPDATE channel_reply_decisions SET state='claimed',attempts=1, \
        last_claimed_at=clock_timestamp(),generation=$3,worker_id=$4,lease_expires_at=clock_timestamp()+interval '800 milliseconds' \
        WHERE company_id=$1 AND run_id=$2 RETURNING proposal_id,integrity,lease_expires_at")
        .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid()).bind(generation).bind(worker.0).fetch_one(f.p.pool()).await.unwrap();
    let live = ReplyPublicationClaim {
        company: f.scope.company,
        run: f.scope.run,
        proposal: crate::domain::workflow::ReplyProposalId::new(row.proposal_id),
        integrity: row.integrity.try_into().unwrap(),
        generation,
        worker,
        attempt: 1,
        expires_at: row.lease_expires_at,
    };
    let plan = super::authority::plan(&f, live).await;
    sqlx::raw_sql("CREATE FUNCTION delay_final_commit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_sleep(1); RETURN NEW; END $$; \
        CREATE TRIGGER delay_final_commit AFTER UPDATE ON channel_reply_decisions FOR EACH ROW WHEN (NEW.state='published') EXECUTE FUNCTION delay_final_commit();")
        .execute(f.p.pool()).await.unwrap();
    let before = outbound_count(&f).await;
    assert!(f.p.commit_reply_publication(live, &plan).await.is_err());
    assert_eq!(outbound_count(&f).await, before);
    assert_eq!(f.state().await["decision"], "claimed");
    sqlx::raw_sql("DROP TRIGGER delay_final_commit ON channel_reply_decisions; DROP FUNCTION delay_final_commit();").execute(f.p.pool()).await.unwrap();
    assert_eq!(
        f.p.commit_reply_publication(live, &plan).await.unwrap(),
        ReplyPublicationOutcome::Lost
    );
    let reclaimed = claim(&f).await;
    assert_eq!(reclaimed.attempt, 2);
    assert!(matches!(
        publisher(&f).publish_claim(reclaimed).await.unwrap(),
        ReplyPublicationOutcome::Published(_)
    ));
    assert_eq!(outbound_count(&f).await, before + 1);
}

#[tokio::test]
async fn channel_reply_publication_and_delivery_survive_original_execution_deadline() {
    let s = Intents::new().await;
    // A short deadline is set at admission, with the real immutable-snapshot guard enabled.
    sqlx::raw_sql("CREATE FUNCTION short_final_run() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN NEW.deadline:=clock_timestamp()+interval '1 second'; RETURN NEW; END $$; \
        CREATE TRIGGER short_final_run BEFORE INSERT ON workflow_runs FOR EACH ROW EXECUTE FUNCTION short_final_run();")
        .execute(s.pool()).await.unwrap();
    let f = FinalFixture::configured(
        s,
        final_source("data.map"),
        literal_contract("Late delivery"),
        false,
    )
    .await;
    assert_eq!(f.advance().await.disposition, BatchDisposition::Completed);
    sqlx::query("SELECT pg_sleep(GREATEST(0,extract(epoch FROM deadline-clock_timestamp()))::double precision+0.01) FROM workflow_runs WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid()).execute(f.p.pool()).await.unwrap();
    let outcome = publisher(&f).publish_claim(claim(&f).await).await.unwrap();
    let ReplyPublicationOutcome::Published(receipt) = outcome else {
        panic!("{outcome:?}")
    };
    let current = super::delivery::claim_delivery(&f, receipt).await;
    assert!(matches!(
        crate::transport::DeliveryQueue::begin_part(&f.p, &current.lease, current.parts[0].id)
            .await
            .unwrap(),
        crate::transport::DeliveryOutcome::Applied(_)
    ));
}
