use super::*;
use crate::application::use_cases::channel::{ChannelPersistence, ChannelWrite};
use crate::domain::entities::message::CanonicalMessageId;
use crate::domain::workflow::{ActionInvocationId, ActionRef, ScheduleId, ScheduleOccurrenceId};
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

struct ForeignSource {
    company: CompanyId,
    channel: Uuid,
    message: Uuid,
}
impl ForeignSource {
    async fn new(f: &AdmissionFixture) -> Self {
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

async fn foreign_parent(f: &AdmissionFixture, company: CompanyId) -> ExecutionRef {
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
async fn workflow_admit_sql_parent_source_checks_run_execution_step_and_action_is_unsupported() {
    let f = AdmissionFixture::new().await;
    let parent = f.prepare(f.request("parent", f.manual())).await;
    f.persistence().admit(&parent).await.unwrap();
    let valid = execution(&f, &parent).await;
    let other = f.prepare(f.request("other-parent", f.manual())).await;
    f.persistence().admit(&other).await.unwrap();
    let foreign = ForeignSource::new(&f).await;
    let foreign = foreign_parent(&f, foreign.company).await;
    for (key, run, execution, step) in [
        (
            "missing-run",
            RunId::new(Uuid::new_v4()),
            valid.execution_id(),
            valid.step_id().clone(),
        ),
        (
            "missing-execution",
            valid.run_id(),
            ExecutionId::new(Uuid::new_v4()),
            valid.step_id().clone(),
        ),
        (
            "wrong-run",
            other.proposed_run_id(),
            valid.execution_id(),
            valid.step_id().clone(),
        ),
        (
            "wrong-step",
            valid.run_id(),
            valid.execution_id(),
            StepId::parse("other").unwrap(),
        ),
        (
            "foreign",
            foreign.run_id(),
            foreign.execution_id(),
            foreign.step_id().clone(),
        ),
    ] {
        let parent = ExecutionRef::new(f.binding.target.company, run, execution, step);
        rejected(
            &f,
            source_request(
                &f,
                key,
                TriggerSource::Child {
                    parent: ChildCause::Execution(parent),
                },
            ),
        )
        .await;
    }
    let action = ActionRef::new(valid.clone(), ActionInvocationId::new(Uuid::new_v4()));
    let command = f
        .prepare(source_request(
            &f,
            "action",
            TriggerSource::Child {
                parent: ChildCause::Action(action),
            },
        ))
        .await;
    let before = f.counts().await;
    assert!(matches!(
        f.persistence().admit(&command).await,
        Err(AppError::BadRequest(_))
    ));
    assert_eq!(f.counts().await, before);
    let command = f
        .prepare(source_request(
            &f,
            "child",
            TriggerSource::Child {
                parent: ChildCause::Execution(valid),
            },
        ))
        .await;
    assert_eq!(
        f.persistence().admit(&command).await.unwrap(),
        AdmissionResult::Created(command.proposed_run_id())
    );
}
