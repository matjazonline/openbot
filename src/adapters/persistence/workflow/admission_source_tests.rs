use super::*;
use crate::application::use_cases::channel::{ChannelPersistence, ChannelWrite};
use crate::domain::entities::message::CanonicalMessageId;
use crate::domain::workflow::{ScheduleId, ScheduleOccurrenceId};
use admission_history_tests::Conversation;

fn source_request(f: &AdmissionFixture, key: &str, source: TriggerSource) -> AdmitWorkflowRequest {
    f.request(
        key,
        TriggerRef::new(
            f.binding.target.company,
            TriggerId::new(Uuid::new_v4()),
            source,
        )
        .unwrap(),
    )
}

async fn rejected(f: &AdmissionFixture, request: AdmitWorkflowRequest) {
    let command = f.prepare(request).await;
    let before = f.counts().await;
    assert!(matches!(
        f.persistence().admit(&command).await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(f.counts().await, before);
}

pub(super) struct ForeignSource {
    pub(super) company: CompanyId,
    channel: Uuid,
    message: Uuid,
}
impl ForeignSource {
    pub(super) async fn new(f: &AdmissionFixture) -> Self {
        let p = f.persistence();
        let company = CompanyPersistence::create(
            p,
            f.binding.target.actor.user_id(),
            CompanyWrite {
                name: "Foreign".into(),
                slug: "foreign".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let channel = ChannelPersistence::create(
            p,
            company.id,
            ChannelWrite {
                name: "Foreign".into(),
                slug: "foreign".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let message = Uuid::new_v4();
        sqlx::query("INSERT INTO messages (id,company_id,author_principal_id,subject,clean_text_body,direction,role,correlation_id,content_hash) SELECT $1,$2,id,'Foreign','Body','inbound','human',$3,$4 FROM principals WHERE company_id=$2 AND user_id=$5")
            .bind(message).bind(company.id).bind(Uuid::new_v4()).bind(vec![0_u8;32]).bind(f.binding.target.actor.user_id()).execute(p.pool()).await.unwrap();
        Self {
            company: CompanyId::new(company.id),
            channel: channel.id,
            message,
        }
    }
}

#[tokio::test]
async fn workflow_admit_sql_message_source_rejects_missing_foreign_and_wrong_membership() {
    let f = AdmissionFixture::new().await;
    let c = Conversation::new(&f).await;
    let other = Conversation::new(&f).await;
    let foreign = ForeignSource::new(&f).await;
    for (key, message) in [
        ("missing", Uuid::new_v4()),
        ("foreign", foreign.message),
        ("other-thread", other.message),
    ] {
        rejected(&f, c.request(&f, key, message)).await;
    }
    rejected(
        &f,
        source_request(
            &f,
            "no-thread",
            TriggerSource::Message {
                message_id: CanonicalMessageId::new(c.message),
            },
        ),
    )
    .await;
    let mut wrong_channel = c.request(&f, "wrong-channel", c.message);
    wrong_channel.association = RelatedAssociation::Thread {
        channel_id: RelatedChannelId::new(other.channel),
        thread_id: RelatedThreadId::new(c.thread),
    };
    rejected(&f, wrong_channel).await;
    let valid = f.prepare(c.request(&f, "valid", c.message)).await;
    assert_eq!(
        f.persistence().admit(&valid).await.unwrap(),
        AdmissionResult::Created(valid.proposed_run_id())
    );
}

struct Occurrence {
    schedule: Uuid,
    occurrence: Uuid,
    channel: Uuid,
}
impl Occurrence {
    async fn new(f: &AdmissionFixture, company: CompanyId, channel: Uuid) -> Self {
        let schedule = Uuid::new_v4();
        let occurrence = Uuid::new_v4();
        sqlx::query("INSERT INTO channel_schedules (id,company_id,channel_id,name,schedule_type,subject_template,prompt_template) VALUES ($1,$2,$3,'Admission','one_off','Subject','Prompt')")
            .bind(schedule).bind(company.as_uuid()).bind(channel).execute(f.persistence().pool()).await.unwrap();
        sqlx::query("INSERT INTO schedule_runs (id,schedule_id,scheduled_for,schedule_snapshot,company_id,channel_id) VALUES ($1,$2,CURRENT_TIMESTAMP,'{}',$3,$4)")
            .bind(occurrence).bind(schedule).bind(company.as_uuid()).bind(channel).execute(f.persistence().pool()).await.unwrap();
        Self {
            schedule,
            occurrence,
            channel,
        }
    }
    fn request(&self, f: &AdmissionFixture, key: &str) -> AdmitWorkflowRequest {
        let mut request = source_request(
            f,
            key,
            TriggerSource::Schedule {
                schedule_id: ScheduleId::new(self.schedule),
                occurrence_id: ScheduleOccurrenceId::new(self.occurrence),
            },
        );
        request.association = RelatedAssociation::Channel(RelatedChannelId::new(self.channel));
        request
    }
}

#[tokio::test]
async fn workflow_admit_sql_schedule_source_checks_occurrence_owner_and_association() {
    let f = AdmissionFixture::new().await;
    let c = Conversation::new(&f).await;
    let foreign = ForeignSource::new(&f).await;
    let first = Occurrence::new(&f, f.binding.target.company, c.channel).await;
    let second = Occurrence::new(&f, f.binding.target.company, c.channel).await;
    let foreign = Occurrence::new(&f, foreign.company, foreign.channel).await;
    for (key, schedule, occurrence) in [
        ("missing-schedule", Uuid::new_v4(), first.occurrence),
        ("missing-occurrence", first.schedule, Uuid::new_v4()),
        ("wrong-owner", second.schedule, first.occurrence),
        ("foreign-owner", foreign.schedule, foreign.occurrence),
        ("foreign-occurrence", first.schedule, foreign.occurrence),
    ] {
        let mut request = source_request(
            &f,
            key,
            TriggerSource::Schedule {
                schedule_id: ScheduleId::new(schedule),
                occurrence_id: ScheduleOccurrenceId::new(occurrence),
            },
        );
        request.association = RelatedAssociation::Channel(RelatedChannelId::new(c.channel));
        rejected(&f, request).await;
    }
    let mut wrong_thread = first.request(&f, "wrong-thread");
    wrong_thread.association = RelatedAssociation::Thread {
        channel_id: RelatedChannelId::new(c.channel),
        thread_id: RelatedThreadId::new(c.thread),
    };
    rejected(&f, wrong_thread).await;
    let valid = f.prepare(first.request(&f, "valid")).await;
    assert_eq!(
        f.persistence().admit(&valid).await.unwrap(),
        AdmissionResult::Created(valid.proposed_run_id())
    );
}

#[tokio::test]
async fn workflow_admit_sql_schedule_source_revocation_blocks_new_replay_and_alias() {
    for key in ["new", "original", "alias"] {
        let f = AdmissionFixture::new().await;
        let c = Conversation::new(&f).await;
        let occurrence = Occurrence::new(&f, f.binding.target.company, c.channel).await;
        let original = f.prepare(occurrence.request(&f, "original")).await;
        if key != "new" {
            f.persistence().admit(&original).await.unwrap();
        }
        let command = if key == "original" {
            original
        } else {
            f.prepare(occurrence.request(&f, key)).await
        };
        let mut revoked = f.persistence().pool().begin().await.unwrap();
        sqlx::query("DELETE FROM schedule_runs WHERE id=$1")
            .bind(occurrence.occurrence)
            .execute(&mut *revoked)
            .await
            .unwrap();
        admission_authority_tests::rejected_after_commit(&f, &command, revoked).await;
    }
}

async fn execution(f: &AdmissionFixture, command: &PreparedAdmission) -> ExecutionRef {
    let (id, step): (Uuid, String) = sqlx::query_as(
        "SELECT id,step_id FROM workflow_executions WHERE company_id=$1 AND run_id=$2",
    )
    .bind(command.company_id().as_uuid())
    .bind(command.proposed_run_id().as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    ExecutionRef::new(
        command.company_id(),
        command.proposed_run_id(),
        ExecutionId::new(id),
        StepId::parse(step).unwrap(),
    )
}

pub(super) async fn foreign_parent(f: &AdmissionFixture, company: CompanyId) -> ExecutionRef {
    let mut draft = f.binding.fixture.save(None);
    draft.target.company = company;
    f.binding.fixture.service().save(draft).await.unwrap();
    let mut publish = f.binding.fixture.publish();
    publish.target.company = company;
    f.binding
        .fixture
        .service()
        .publish(publish.clone())
        .await
        .unwrap();
    let mut binding = f.binding.request(None);
    binding.target.company = company;
    binding.target.binding = WorkflowBindingId::new(Uuid::new_v4());
    binding.version = publish.version;
    let target = binding.target;
    let configured = f.binding.service().configure(binding).await.unwrap();
    f.binding
        .service()
        .activate(SetBindingActivity {
            target,
            expected: configured.revision,
        })
        .await
        .unwrap();
    let mut request = f.request(
        "foreign-parent",
        TriggerRef::new(
            company,
            TriggerId::new(Uuid::new_v4()),
            TriggerSource::Manual,
        )
        .unwrap(),
    );
    request.company_id = company;
    request.binding_id = target.binding;
    let command = f.prepare(request).await;
    f.persistence().admit(&command).await.unwrap();
    execution(f, &command).await
}

#[tokio::test]
async fn workflow_admit_refuses_every_child_trigger_and_schema_checks_runtime_child_keys() {
    // Category C (07-D1 §D1.2a): no admission creates a child, whatever its parent cause.
    let f = AdmissionFixture::new().await;
    let parent = f.prepare(f.request("parent", f.manual())).await;
    f.persistence().admit(&parent).await.unwrap();
    let valid = execution(&f, &parent).await;
    let missing = ExecutionRef::new(
        f.binding.target.company,
        RunId::new(Uuid::new_v4()),
        ExecutionId::new(Uuid::new_v4()),
        valid.step_id().clone(),
    );
    let call = ChildToolCallRef::new(valid.clone(), ToolCallId::new(Uuid::new_v4()));
    let p = f.persistence().clone();
    let service = WorkflowService::new(p.clone(), p, NoRuntime, NoRuntime, Preflight);
    for (key, cause) in [
        (
            "valid",
            ChildCause::Step(ChildStepRef::new(valid.clone(), ChildOrdinal::FIRST)),
        ),
        (
            "missing",
            ChildCause::Step(ChildStepRef::new(missing, ChildOrdinal::FIRST)),
        ),
        ("tool-call", ChildCause::ToolCall(call)),
    ] {
        let before = f.counts().await;
        let result = service
            .admit(source_request(
                &f,
                key,
                TriggerSource::Child { parent: cause },
            ))
            .await;
        assert!(
            matches!(result, Err(AppError::BadRequest(_))),
            "{key}: {result:?}"
        );
        assert_eq!(f.counts().await, before);
    }

    // The schema accepts only the runtime child key of the child's own parent execution.
    let (f, scopes) = AdmissionFixture::chain(
        serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap(),
        1,
    )
    .await;
    let (root, child) = (scopes[0], scopes[1]);
    let key = |step: &str, action: Value, ordinal: Value| {
        format!(
            "v1:{}",
            json!([
                "child",
                root.run.as_uuid(),
                root.execution.as_uuid(),
                step,
                action,
                ordinal
            ])
        )
    };
    let five = format!(
        "v1:{}",
        json!([
            "child",
            root.run.as_uuid(),
            root.execution.as_uuid(),
            "start",
            null
        ])
    );
    let insert = |source: String| {
        let f = &f;
        async move {
            sqlx::query("INSERT INTO workflow_admissions (company_id, binding_id, source_key, run_id) SELECT company_id, binding_id, $2, id FROM workflow_runs WHERE id=$1")
                .bind(child.run.as_uuid()).bind(source).execute(f.persistence().pool()).await
                .unwrap_err().as_database_error().unwrap().code().unwrap().into_owned()
        }
    };
    for source in [
        five,
        key("start", json!(null), json!(0)),
        key("start", json!(null), json!(1001)),
        key("start", json!(null), json!(1.5)),
        key("start", json!(null), json!("1")),
        key("start", json!(Uuid::new_v4()), json!(1)),
        key("other", json!(null), json!(1)),
    ] {
        assert_eq!(insert(source.clone()).await, "23514", "{source}");
    }
    // The real key passes the trigger and meets the child's existing admission.
    assert_eq!(insert(key("start", json!(null), json!(1))).await, "23505");
}

#[tokio::test]
async fn workflow_control_head_restores_real_message_and_schedule_causality() {
    let f = AdmissionFixture::new().await;
    let c = Conversation::new(&f).await;
    let occurrence = Occurrence::new(&f, f.binding.target.company, c.channel).await;
    for request in [
        c.request(&f, "message-head", c.message),
        occurrence.request(&f, "schedule-head"),
    ] {
        let trigger = request.trigger.clone();
        let association = request.association;
        let correlation = request.correlation_id;
        let command = f.prepare(request).await;
        f.persistence().admit(&command).await.unwrap();
        let head = f
            .persistence()
            .head(command.company_id(), command.proposed_run_id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(head.causality.trigger(), &trigger);
        assert_eq!(head.causality.correlation_id(), correlation);
        assert_eq!(head.association, association);
        assert_eq!(head.causality.run_id(), command.proposed_run_id());
        let cancel = CancelCommand {
            company_id: command.company_id(),
            run_id: command.proposed_run_id(),
            actor: f.binding.target.actor,
            command_key: IdempotencyKey::parse(format!(
                "control-{}",
                command.proposed_run_id().as_uuid()
            ))
            .unwrap(),
            expected_revision: head.revision,
        };
        assert!(matches!(
            f.persistence().cancel(cancel).await.unwrap(),
            CancelResult::Applied { .. }
        ));
        let saved = f
            .persistence()
            .head(command.company_id(), command.proposed_run_id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.causality, head.causality);
        assert_eq!(saved.association, head.association);
    }
}
