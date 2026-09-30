//! Matrix-local actor grants and authoritative verification instrumentation.
use super::*;

#[path = "action_reconciliation_directory_tests.rs"]
mod directory_tests;

#[derive(Clone)]
pub(super) struct ActorResources(PostgresPersistence);
impl ActorResources {
    fn status(action: &FrozenAction, authorized: bool) -> ResourceStatus {
        ResourceStatus {
            company_id: action.scope().company,
            id: RuntimeResourceId::new(action.scope().execution.as_uuid()),
            kind: TypeName::parse("fixture").unwrap(),
            authorized,
            readiness: ResourceReadiness::Ready,
            supported_contracts: [action.request().contract.contract.name.clone()].into(),
        }
    }
}
#[async_trait]
impl SqlReconciliationResources for ActorResources {
    async fn lock_resource(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: WorkflowActor,
        frozen: &ActionRunAuthority,
    ) -> AppResult<ResourceStatus> {
        let enabled: bool = sqlx::query_scalar("SELECT enabled FROM fixture_actor_resources WHERE company_id=$1 AND user_id=$2 FOR SHARE")
            .bind(frozen.action.scope().company.as_uuid()).bind(actor.user_id())
            .fetch_optional(&mut **tx).await?.unwrap_or(false);
        Ok(Self::status(&frozen.action, enabled))
    }
}
#[async_trait]
impl ResourceDirectory for ActorResources {
    async fn inspect(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        id: RuntimeResourceId,
    ) -> AppResult<Option<ResourceStatus>> {
        let enabled: bool = sqlx::query_scalar(
            "SELECT enabled FROM fixture_actor_resources WHERE company_id=$1 AND user_id=$2",
        )
        .bind(company.as_uuid())
        .bind(actor.user_id())
        .fetch_optional(self.0.pool())
        .await?
        .unwrap_or(false);
        Ok(Some(ResourceStatus {
            company_id: company,
            id,
            kind: TypeName::parse("fixture").unwrap(),
            authorized: enabled,
            readiness: ResourceReadiness::Ready,
            supported_contracts: [TypeName::parse("fixture.write").unwrap()].into(),
        }))
    }
}
#[async_trait]
impl SqlActionAuthority for ActorResources {
    async fn lock_current(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        saved: &ActionRunAuthority,
    ) -> AppResult<LockedActionAuthority> {
        let mut current = Authority.lock_current(tx, saved).await?;
        current.resource = self.lock_resource(tx, saved.actor, saved).await?;
        Ok(current)
    }
}

pub(super) async fn resources(f: &AdmissionFixture) -> ActorResources {
    sqlx::raw_sql("CREATE TABLE fixture_actor_resources(company_id uuid NOT NULL,user_id uuid NOT NULL,enabled bool NOT NULL,PRIMARY KEY(company_id,user_id))")
        .execute(f.persistence().pool()).await.unwrap();
    grant(f, f.binding.target.actor, true).await;
    ActorResources(f.persistence().clone())
}
pub(super) async fn grant(f: &AdmissionFixture, actor: WorkflowActor, enabled: bool) {
    sqlx::query("INSERT INTO fixture_actor_resources VALUES($1,$2,$3) ON CONFLICT(company_id,user_id) DO UPDATE SET enabled=EXCLUDED.enabled")
        .bind(f.binding.target.company.as_uuid()).bind(actor.user_id()).bind(enabled)
        .execute(f.persistence().pool()).await.unwrap();
}
pub(super) async fn principal(f: &AdmissionFixture, role: Option<&str>) -> WorkflowActor {
    let user = Uuid::new_v4();
    let suffix = user.simple().to_string();
    sqlx::query("INSERT INTO users(id,username,email,password_hash) VALUES($1,$2,$3,'test')")
        .bind(user)
        .bind(&suffix)
        .bind(format!("{suffix}@example.test"))
        .execute(f.persistence().pool())
        .await
        .unwrap();
    if let Some(role) = role {
        sqlx::query("INSERT INTO company_members(id,company_id,user_id,role) VALUES($1,$2,$3,$4)")
            .bind(Uuid::new_v4())
            .bind(f.binding.target.company.as_uuid())
            .bind(user)
            .bind(role)
            .execute(f.persistence().pool())
            .await
            .unwrap();
        sqlx::query("INSERT INTO principals(id,company_id,kind,user_id,display_label) VALUES($1,$2,'person',$3,'matrix')")
            .bind(Uuid::new_v4()).bind(f.binding.target.company.as_uuid()).bind(user)
            .execute(f.persistence().pool()).await.unwrap();
    }
    let actor = WorkflowActor::authenticated(user).unwrap();
    grant(f, actor, true).await;
    actor
}
pub(super) struct ObservedVerifier {
    pub(super) ledger: Arc<LedgerVerifier>,
    pub(super) calls: AtomicUsize,
    pub(super) pause: Option<(Arc<Notify>, Arc<Notify>)>,
}
impl ObservedVerifier {
    pub(super) fn new(ledger: Arc<LedgerVerifier>) -> Self {
        Self {
            ledger,
            calls: AtomicUsize::new(0),
            pause: None,
        }
    }
}
#[async_trait]
impl ActionEvidenceVerifier for ObservedVerifier {
    fn registration(&self) -> &EvidenceVerifierRegistration {
        self.ledger.registration()
    }
    async fn verify(
        &self,
        snapshot: &ReconciliationSnapshot,
        reference: &EvidenceRecordReference,
        cancellation: &CancellationToken,
    ) -> AppResult<EvidenceAttestation> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let evidence = self
            .ledger
            .verify(snapshot, reference, cancellation)
            .await?;
        if let Some((started, release)) = &self.pause {
            started.notify_one();
            tokio::select! {
                _ = cancellation.cancelled() => return Err(AppError::Conflict("fixture cancelled".into())),
                _ = release.notified() => {}
            }
        }
        Ok(evidence)
    }
}
pub(super) async fn execute(
    f: &AdmissionFixture,
    command: &ReconcileActionCommand,
    verifier: Arc<dyn ActionEvidenceVerifier>,
) -> AppResult<ReconciliationResult> {
    let p = f.persistence().clone();
    let r = ActorResources(p.clone());
    ActionReconciliationService::new(
        PostgresActionReconciliation::new(p.clone(), r.clone()),
        LifecycleAuthorizer::new(p.clone(), p.clone(), p),
        r,
        Some(verifier),
    )
    .reconcile(command, &CancellationToken::new(), Duration::from_secs(5))
    .await
}
pub(super) async fn verified_command(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    verifier: &ObservedVerifier,
    key: &str,
) -> ReconcileActionCommand {
    let mut c = command(f, request, marker(f).await).await;
    c.command_key = IdempotencyKey::parse(key).unwrap();
    c.input = EvidenceInput::VerifiedReference {
        registration: verifier.registration().id().clone(),
        reference: EvidenceRecordReference::parse(key).unwrap(),
    };
    c
}
pub(super) async fn durable(f: &AdmissionFixture) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('runs',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM workflow_runs AS r),'jobs',(SELECT jsonb_agg(to_jsonb(j) ORDER BY id) FROM background_tasks AS j),'executions',(SELECT jsonb_agg(to_jsonb(e) ORDER BY id) FROM workflow_executions AS e),'attempts',(SELECT jsonb_agg(to_jsonb(a) ORDER BY id) FROM task_attempts AS a),'budget',(SELECT jsonb_agg(to_jsonb(b) ORDER BY to_jsonb(b)::text) FROM workflow_budget_receipts AS b),'usage',(SELECT jsonb_agg(to_jsonb(u) ORDER BY to_jsonb(u)::text) FROM workflow_root_budget_usage AS u),'commands',(SELECT jsonb_agg(to_jsonb(c) ORDER BY id) FROM workflow_action_evidence_commands AS c),'evidence',(SELECT jsonb_agg(to_jsonb(v) ORDER BY id) FROM workflow_action_evidence AS v),'events',(SELECT jsonb_agg(to_jsonb(v) ORDER BY sequence) FROM workflow_run_events AS v),'entries',(SELECT jsonb_agg(to_jsonb(v) ORDER BY id) FROM workflow_action_remote_entries AS v))")
        .fetch_one(f.persistence().pool()).await.unwrap()
}
pub(super) fn same_execution(before: &Value, after: &Value) {
    for key in [
        "jobs",
        "executions",
        "attempts",
        "budget",
        "usage",
        "entries",
    ] {
        assert_eq!(before[key], after[key], "{key}");
    }
}
pub(super) async fn prepared() -> (
    AdmissionFixture,
    ActionDispatchRequest,
    Arc<ObservedVerifier>,
) {
    let (f, request) = setup().await;
    resources(&f).await;
    let verifier = Arc::new(ObservedVerifier::new(ledger(&f, &request).await));
    let provider = LedgerProvider::new(&f, Delivery::Pending);
    assert!(invoke(&f, &request, &provider).await.is_err());
    park(&f, &request).await;
    (f, request, verifier)
}

pub(super) async fn successful_replay(
    f: &AdmissionFixture,
    c: &ReconcileActionCommand,
    verifier: Arc<ObservedVerifier>,
    saved: &ReconciliationResult,
) {
    let before = durable(f).await;
    let replay = execute(f, c, verifier.clone()).await.unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.outcome, saved.outcome);
    assert_eq!(replay.revision, saved.revision);
    assert_eq!(replay.evidence, saved.evidence);
    assert!(saved.revision.0 > c.expected_revision.0);
    for variant in 0..4 {
        let mut changed = c.clone();
        match variant {
            0 => changed.actor = f.binding.target.actor,
            1 => changed.expected_revision = saved.revision,
            2 => {
                changed.input = EvidenceInput::UnknownNote {
                    note: "different input".into(),
                    claimed: ClaimedDisposition::Applied,
                }
            }
            _ => {
                changed.input = EvidenceInput::VerifiedReference {
                    registration: verifier.registration().id().clone(),
                    reference: EvidenceRecordReference::parse("different-reference").unwrap(),
                }
            }
        }
        assert_eq!(
            execute(f, &changed, verifier.clone())
                .await
                .unwrap()
                .outcome,
            ReconciliationOutcome::IdempotencyConflict
        );
        assert_eq!(durable(f).await, before);
    }
    assert_eq!(durable(f).await, before);
    assert_eq!(verifier.calls.load(Ordering::SeqCst), 1);
    grant(f, c.actor, false).await;
    assert!(execute(f, c, verifier.clone()).await.is_err());
    assert_eq!(durable(f).await, before);
    grant(f, c.actor, true).await;
    sqlx::query("UPDATE company_members SET role='member' WHERE user_id=$1")
        .bind(c.actor.user_id())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    assert!(matches!(
        execute(f, c, verifier.clone()).await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(durable(f).await, before);
    sqlx::query("UPDATE company_members SET role='admin' WHERE user_id=$1")
        .bind(c.actor.user_id())
        .execute(f.persistence().pool())
        .await
        .unwrap();
}
pub(super) async fn mutate_authority(f: &AdmissionFixture, admin: WorkflowActor, mutation: &str) {
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM companies FOR UPDATE")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM workflow_runs FOR UPDATE")
        .execute(&mut *tx)
        .await
        .unwrap();
    match mutation {
        "resource" => {
            sqlx::query("UPDATE fixture_actor_resources SET enabled=false WHERE user_id=$1")
                .bind(admin.user_id())
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        "principal" => {
            sqlx::query("UPDATE company_members SET role='member' WHERE user_id=$1")
                .bind(admin.user_id())
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        _ => {
            sqlx::query("UPDATE workflow_runs SET deadline=deadline+interval '1 second'")
                .execute(&mut *tx)
                .await
                .unwrap();
        }
    }
    tx.commit().await.unwrap();
}
