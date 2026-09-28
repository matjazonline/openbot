use super::*;
use crate::application::use_cases::channel::{ChannelPersistence, ChannelWrite};
use crate::domain::entities::message::CanonicalMessageId;

pub(super) struct Conversation {
    pub(super) channel: Uuid,
    pub(super) thread: Uuid,
    pub(super) message: Uuid,
}
impl Conversation {
    pub(super) async fn new(f: &AdmissionFixture) -> Self {
        let p = f.persistence();
        let company = f.binding.target.company.as_uuid();
        let channel = ChannelPersistence::create(
            p,
            company,
            ChannelWrite {
                name: "Admission".into(),
                slug: format!("admission-{}", Uuid::new_v4().simple()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let conversation = Self {
            channel: channel.id,
            thread: Uuid::new_v4(),
            message: Uuid::new_v4(),
        };
        sqlx::query("INSERT INTO threads (id, company_id, channel_id, subject) VALUES ($1,$2,$3,'Admission')")
            .bind(conversation.thread).bind(company).bind(channel.id).execute(p.pool()).await.unwrap();
        let mut db = p.pool().acquire().await.unwrap();
        conversation.insert(&mut db, f, conversation.message).await;
        conversation
    }
    pub(super) async fn insert(&self, db: &mut PgConnection, f: &AdmissionFixture, message: Uuid) {
        self.insert_message(db, f, message).await;
        self.insert_membership(db, f, message).await;
    }
    async fn insert_message(&self, db: &mut PgConnection, f: &AdmissionFixture, message: Uuid) {
        let company = f.binding.target.company.as_uuid();
        sqlx::query("INSERT INTO messages (id, company_id, author_principal_id, subject, clean_text_body, direction, role, correlation_id, content_hash) SELECT $1,$2,id,'History','body','inbound','human',$3,$4 FROM principals WHERE company_id=$2 AND user_id=$5")
            .bind(message).bind(company).bind(Uuid::new_v4()).bind(vec![0_u8;32])
            .bind(f.binding.target.actor.user_id()).execute(&mut *db).await.unwrap();
    }
    async fn insert_membership(&self, db: &mut PgConnection, f: &AdmissionFixture, message: Uuid) {
        let company = f.binding.target.company.as_uuid();
        sqlx::query("INSERT INTO thread_messages (id,company_id,channel_id,thread_id,message_id,entry_kind,created_at) VALUES ($1,$2,$3,$4,$5,'conversation',CURRENT_TIMESTAMP-interval '1 day')")
            .bind(Uuid::new_v4()).bind(company).bind(self.channel).bind(self.thread).bind(message).execute(db).await.unwrap();
    }
    pub(super) fn request(
        &self,
        f: &AdmissionFixture,
        key: &str,
        message: Uuid,
    ) -> AdmitWorkflowRequest {
        let trigger = TriggerRef::new(
            f.binding.target.company,
            TriggerId::new(Uuid::new_v4()),
            TriggerSource::Message {
                message_id: CanonicalMessageId::new(message),
            },
        )
        .unwrap();
        let mut request = f.request(key, trigger);
        request.association = RelatedAssociation::Thread {
            channel_id: RelatedChannelId::new(self.channel),
            thread_id: RelatedThreadId::new(self.thread),
        };
        request
    }
    async fn members(&self, f: &AdmissionFixture, run: RunId) -> Vec<Uuid> {
        sqlx::query_scalar("SELECT message_id FROM workflow_history_members WHERE company_id=$1 AND run_id=$2 ORDER BY ordinal")
            .bind(f.binding.target.company.as_uuid()).bind(run.as_uuid()).fetch_all(f.persistence().pool()).await.unwrap()
    }
}

#[tokio::test]
async fn workflow_admit_sql_message_alias_equivalence_and_independent_runs() {
    let f = AdmissionFixture::new().await;
    let c = Conversation::new(&f).await;
    let first = f.prepare(c.request(&f, "first", c.message)).await;
    let second = f.prepare(c.request(&f, "redelivery", c.message)).await;
    let p = f.persistence();
    let (a, b) = tokio::join!(p.admit(&first), p.admit(&second));
    let results = [a.unwrap(), b.unwrap()];
    let run = results
        .iter()
        .find_map(|result| match result {
            AdmissionResult::Created(run) => Some(*run),
            _ => None,
        })
        .unwrap();
    assert!(results.contains(&AdmissionResult::Replayed(run)));
    assert_eq!(c.members(&f, run).await, vec![c.message]);
    assert_eq!(
        p.admit(&second).await.unwrap(),
        AdmissionResult::Replayed(run)
    );
    // Bypass the binding reader to exercise the atomic writer's own command check.
    let mut altered = c.request(&f, "other-key", c.message);
    altered.idempotency_key = first.idempotency_key().clone();
    let capture = Capture::default();
    struct Selected(Arc<binding::ConfiguredBinding>);
    #[async_trait]
    impl WorkflowBindings for Selected {
        async fn admission_binding(
            &self,
            _: CompanyId,
            _: WorkflowBindingId,
            _: &IdempotencyKey,
            _: &TriggerRef,
        ) -> AppResult<Option<Arc<binding::ConfiguredBinding>>> {
            Ok(Some(self.0.clone()))
        }
    }
    WorkflowService::new(
        Selected(first.binding().clone()),
        capture.clone(),
        NoRuntime,
        NoRuntime,
        Preflight,
    )
    .admit(altered)
    .await
    .unwrap();
    let changed_trigger = capture.0.lock().unwrap().take().unwrap();
    assert_eq!(
        p.admit(&changed_trigger).await.unwrap(),
        AdmissionResult::Conflict
    );
    let next_message = Uuid::new_v4();
    c.insert(&mut p.pool().acquire().await.unwrap(), &f, next_message)
        .await;
    let next = f.prepare(c.request(&f, "next", next_message)).await;
    assert_eq!(
        p.admit(&next).await.unwrap(),
        AdmissionResult::Created(next.proposed_run_id())
    );
    assert_eq!(c.members(&f, run).await, vec![c.message]);
    assert_eq!(c.members(&f, next.proposed_run_id()).await.len(), 2);
    assert_eq!(f.counts().await["jobs"], 2);
}

async fn await_admission_audit_lock(pool: &sqlx::PgPool) {
    tokio::time::timeout(std::time::Duration::from_secs(10),async {
        loop {
            let blocked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE 'INSERT INTO workflow_run_events%')")
                .fetch_one(pool).await.unwrap();
            if blocked { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.expect("admission must reach the audit after history capture");
}

#[tokio::test]
async fn workflow_admit_sql_history_excludes_older_uncommitted_membership() {
    let f = AdmissionFixture::new().await;
    let c = Conversation::new(&f).await;
    let p = f.persistence();
    let command = f
        .prepare(c.request(&f, "before-late-commit", c.message))
        .await;
    let late_message = Uuid::new_v4();
    // Canonical content is committed first; only membership is concurrent.
    // A message INSERT itself holds a company FK lock, preventing admission's
    // company authority lock from reaching the capture boundary at all.
    c.insert_message(&mut p.pool().acquire().await.unwrap(), &f, late_message)
        .await;
    let mut late = p.pool().begin().await.unwrap();
    c.insert_membership(&mut late, &f, late_message).await;
    sqlx::query("CREATE FUNCTION pause_admission_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_advisory_xact_lock(914037); RETURN NEW; END; $$")
        .execute(p.pool()).await.unwrap();
    sqlx::query("CREATE TRIGGER pause_admission BEFORE INSERT ON workflow_run_events FOR EACH ROW EXECUTE FUNCTION pause_admission_audit()")
        .execute(p.pool()).await.unwrap();
    let mut blocker = p.pool().begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(914037)")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let worker = p.clone();
    let pending = command.clone();
    let admission = tokio::spawn(async move { worker.admit(&pending).await });
    await_admission_audit_lock(p.pool()).await;
    late.commit().await.unwrap();
    blocker.commit().await.unwrap();
    assert_eq!(
        admission.await.unwrap().unwrap(),
        AdmissionResult::Created(command.proposed_run_id())
    );
    assert_eq!(
        c.members(&f, command.proposed_run_id()).await,
        vec![c.message]
    );
    let next = f
        .prepare(c.request(&f, "after-late-commit", late_message))
        .await;
    p.admit(&next).await.unwrap();
    assert_eq!(c.members(&f, next.proposed_run_id()).await.len(), 2);
}

#[tokio::test]
async fn workflow_admit_sql_history_overflow_rolls_back_the_admission() {
    let f = AdmissionFixture::new().await;
    let c = Conversation::new(&f).await;
    let p = f.persistence();
    // One existing membership plus the full bound gives exactly bound+1.
    sqlx::query("WITH inserted AS (INSERT INTO messages (id,company_id,author_principal_id,subject,clean_text_body,direction,role,correlation_id,content_hash) SELECT gen_random_uuid(),$1,principal.id,'History','body','inbound','human',gen_random_uuid(),$5 FROM principals AS principal CROSS JOIN generate_series(1,$4::integer) WHERE principal.company_id=$1 AND principal.user_id=$6 RETURNING id) INSERT INTO thread_messages (id,company_id,channel_id,thread_id,message_id,entry_kind) SELECT gen_random_uuid(),$1,$2,$3,id,'conversation' FROM inserted")
        .bind(f.binding.target.company.as_uuid()).bind(c.channel).bind(c.thread)
        .bind(i32::try_from(admission_history::MAX_HISTORY_MEMBERS).unwrap()).bind(vec![0_u8;32])
        .bind(f.binding.target.actor.user_id()).execute(p.pool()).await.unwrap();
    let command = f.prepare(c.request(&f, "overflow", c.message)).await;
    assert!(matches!(
        p.admit(&command).await,
        Err(AppError::BadRequest(_))
    ));
    assert_eq!(
        f.counts().await,
        json!({"runs":0,"executions":0,"jobs":0,"admissions":0,"commands":0,"events":0,"history":0})
    );
}

#[tokio::test]
async fn workflow_admit_sql_replay_rejects_changed_association_for_command_and_alias() {
    let f = AdmissionFixture::new().await;
    let c = Conversation::new(&f).await;
    let trigger = f.manual();
    let first = f.prepare(f.request("original", trigger.clone())).await;
    f.persistence().admit(&first).await.unwrap();
    for key in ["original", "alias"] {
        let mut request = f.request(key, trigger.clone());
        request.association = RelatedAssociation::Thread {
            channel_id: RelatedChannelId::new(c.channel),
            thread_id: RelatedThreadId::new(c.thread),
        };
        let changed = f.prepare(request).await;
        assert_eq!(
            f.persistence().admit(&changed).await.unwrap(),
            AdmissionResult::Conflict
        );
    }
    assert_eq!(f.counts().await["commands"], 1);
    assert_eq!(f.counts().await["jobs"], 1);
}

#[tokio::test]
async fn workflow_admit_sql_binding_scope_narrowing_matrix() {
    let mut f = AdmissionFixture::new().await;
    let a = Conversation::new(&f).await;
    let b = Conversation::new(&f).await;
    let scopes = [
        RelatedAssociation::Company,
        RelatedAssociation::Channel(RelatedChannelId::new(a.channel)),
        RelatedAssociation::Thread {
            channel_id: RelatedChannelId::new(a.channel),
            thread_id: RelatedThreadId::new(a.thread),
        },
        RelatedAssociation::Channel(RelatedChannelId::new(b.channel)),
        RelatedAssociation::Thread {
            channel_id: RelatedChannelId::new(b.channel),
            thread_id: RelatedThreadId::new(b.thread),
        },
    ];
    let allowed = [
        [true, true, true, true, true],
        [false, true, true, false, false],
        [false, false, true, false, false],
        [false, false, false, true, true],
        [false, false, false, false, true],
    ];
    for (binding_index, binding_scope) in scopes.iter().enumerate() {
        f.binding.target.binding = WorkflowBindingId::new(Uuid::new_v4());
        let mut configure = f.binding.request(None);
        configure.association = *binding_scope;
        let configured = f.binding.service().configure(configure).await.unwrap();
        f.binding
            .service()
            .activate(SetBindingActivity {
                target: f.binding.target,
                expected: configured.revision,
            })
            .await
            .unwrap();
        for (run_index, run_scope) in scopes.iter().enumerate() {
            let mut request = f.request(&format!("scope-{binding_index}-{run_index}"), f.manual());
            request.association = *run_scope;
            let command = f.prepare(request).await;
            let result = f.persistence().admit(&command).await;
            if allowed[binding_index][run_index] {
                assert_eq!(
                    result.unwrap(),
                    AdmissionResult::Created(command.proposed_run_id())
                );
            } else {
                assert!(
                    matches!(result, Err(AppError::Conflict(_))),
                    "{binding_scope:?} -> {run_scope:?}"
                );
            }
        }
    }
    assert_eq!(f.counts().await["runs"], 11);
    assert_eq!(f.counts().await["jobs"], 11);
}
