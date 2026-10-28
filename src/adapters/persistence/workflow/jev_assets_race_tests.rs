//! Competing revocation during the final credential transaction's connection wait.
use super::*;
use tokio::sync::Notify;

struct FinalGate {
    persistence: PostgresPersistence,
    entered: Arc<Notify>,
    resume: Arc<Notify>,
}
#[async_trait]
impl JevAuthorization for FinalGate {
    async fn authorize_jev(&self, claim: &ClaimedWorkflow) -> AppResult<JevCredential> {
        self.entered.notify_one();
        self.resume.notified().await;
        self.persistence.authorize_jev(claim).await
    }
}
#[async_trait]
impl JevExecutions for FinalGate {
    async fn replay_jev(
        &self,
        claim: &ClaimedWorkflow,
        request: &JevRequest,
        policy: LeasePolicy,
    ) -> AppResult<Option<WorkflowHandlerResult>> {
        self.persistence.replay_jev(claim, request, policy).await
    }
    async fn admit_jev(
        &self,
        claim: &ClaimedWorkflow,
        request: &JevRequest,
        policy: LeasePolicy,
    ) -> AppResult<JevAdmissionResult> {
        self.persistence.admit_jev(claim, request, policy).await
    }
    async fn settle_jev(
        &self,
        claim: &ClaimedWorkflow,
        admission: &JevAdmission,
        request: &JevRequest,
        response: &JevResponse,
        latency: Duration,
        policy: LeasePolicy,
    ) -> AppResult<()> {
        self.persistence
            .settle_jev(claim, admission, request, response, latency, policy)
            .await
    }
    async fn fail_jev(
        &self,
        claim: &ClaimedWorkflow,
        admission: &JevAdmission,
        failure: JevError,
        latency: Duration,
        policy: LeasePolicy,
    ) -> AppResult<()> {
        self.persistence
            .fail_jev(claim, admission, failure, latency, policy)
            .await
    }
}
async fn restrict_to_view_grant(f: &Fixture, attachment: &Attachment) -> Uuid {
    let owner = Fixture::in_database(f._db.clone(), "race-owner").await;
    let p = &f.persistence;
    let company = f.target.company.as_uuid();
    let channel: Uuid = attachment.reference["channel_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    sqlx::query("UPDATE companies SET user_id=$2 WHERE id=$1")
        .bind(company)
        .bind(owner.target.actor.user_id())
        .execute(p.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE company_members SET role='admin' WHERE company_id=$1 AND user_id=$2")
        .bind(company)
        .bind(f.target.actor.user_id())
        .execute(p.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE channels SET access_mode='allowlist' WHERE company_id=$1 AND id=$2")
        .bind(company)
        .bind(channel)
        .execute(p.pool())
        .await
        .unwrap();
    let principal: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND user_id=$2")
            .bind(company)
            .bind(f.target.actor.user_id())
            .fetch_one(p.pool())
            .await
            .unwrap();
    sqlx::query("INSERT INTO channel_principal_grants(company_id,channel_id,principal_id,capability,provenance) VALUES($1,$2,$3,'view','configured_allowlist')").bind(company).bind(channel).bind(principal).execute(p.pool()).await.unwrap();
    channel
}
async fn blocked_by(pool: &sqlx::PgPool, blocker: i32) -> i32 {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let pid: Option<i32> = sqlx::query_scalar("SELECT pid FROM pg_stat_activity WHERE datname=current_database() AND $1=ANY(pg_blocking_pids(pid)) LIMIT 1").bind(blocker).fetch_optional(pool).await.unwrap();
            if let Some(pid)=pid { return pid }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.expect("competing operation must be blocked by the held authority lock")
}
#[tokio::test]
async fn jev_assets_final_authorization_holds_view_grant_across_connection_wait() {
    let f = fixture().await;
    let bytes = images::png(280, 280);
    let attachment = attachment(&f, &bytes).await;
    let claim = image_claim(&f, &attachment).await;
    let channel = restrict_to_view_grant(&f, &attachment).await;
    let storage = Arc::new(FakeStorage::new());
    storage
        .store_object(BucketKind::Private, &attachment.key, "image/png", bytes)
        .await
        .unwrap();
    let reader = Arc::new(JevAssetReader::new(f.persistence.clone(), Some(storage)));
    let (http, server) = server(
        200,
        "",
        serde_json::to_vec(&images::output()).unwrap(),
        Duration::ZERO,
    )
    .await;
    let entered = Arc::new(Notify::new());
    let resume = Arc::new(Notify::new());
    let runtime = JevRuntime::new(
        FinalGate {
            persistence: f.persistence.clone(),
            entered: entered.clone(),
            resume: resume.clone(),
        },
        Arc::new(http),
        policy(),
    )
    .with_assets(reader);
    let owned = claim.clone();
    let worker = tokio::spawn(async move {
        runtime
            .execute(&WorkflowStepKind("jev.evaluate".into()), &owned)
            .await
    });
    entered.notified().await; // Admission finished; only final authorization is gated.
    let mut connection_lock = f.persistence.pool().begin().await.unwrap();
    let holder: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *connection_lock)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM company_typesafe_connections WHERE company_id=$1 FOR UPDATE")
        .bind(f.target.company.as_uuid())
        .fetch_one(&mut *connection_lock)
        .await
        .unwrap();
    resume.notify_one();
    let authorizer = blocked_by(f.persistence.pool(), holder).await;
    let mut revoker = f.persistence.pool().acquire().await.unwrap();
    let revoker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *revoker)
        .await
        .unwrap();
    let company = f.target.company.as_uuid();
    let revoke = tokio::spawn(async move {
        sqlx::query("DELETE FROM channel_principal_grants WHERE company_id=$1 AND channel_id=$2 AND capability='view'").bind(company).bind(channel).execute(&mut *revoker).await.unwrap().rows_affected()
    });
    assert_eq!(
        blocked_by(f.persistence.pool(), authorizer).await,
        revoker_pid
    );
    assert!(!revoke.is_finished());
    connection_lock.commit().await.unwrap();
    assert_eq!(revoke.await.unwrap(), 1);
    assert!(matches!(
        worker.await.unwrap(),
        WorkflowHandlerOutcome::Completed(Ok(_))
    ));
    assert!(
        server
            .await
            .unwrap()
            .contains("typesafe-danger-zone-images: true")
    );
    assert_eq!(calls(&f.persistence, &claim).await, 1);
    assert!(f.persistence.authorize_jev(&claim).await.is_err());
}
