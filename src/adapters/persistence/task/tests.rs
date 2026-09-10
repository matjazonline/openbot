use super::*;
use crate::use_cases::thread::test_support::{EmailMessageDraft, email_write};
use chrono::{DateTime, Utc};
use sqlx::postgres::types::PgInterval;
use std::str::FromStr;
use std::time::Duration;
use uuid::Uuid;

use crate::adapters::persistence::test_support::{
    DeliveryFixtureRequest, UNSCOPED_CLAIM, delivery_fixture, test_machine, test_pool,
};
use crate::app_error::AppError;
use crate::entities::message::{MessageDirection, MessageRole};
use crate::entities::response_draft::ExternalResponseReview;
use crate::entities::task::TaskFailureOutcome;
use crate::entities::transport::{DeliveryPurpose, DeliveryStatus};
use crate::task_queue::{AgentReviewCandidate, CollaborationReadScope, HumanTaskCompletion};
use crate::transport::{DeliveryCreation, NewDelivery};
use crate::use_cases::response_review::{
    DraftPublicationSnapshot, PreparedReviewDraft, ResponseReviewPersistence, ReviewAction,
    ReviewCommand,
};
use crate::{
    adapters::persistence::PostgresPersistence,
    entities::{
        correlation::CorrelationId,
        delegation::{
            DelegationActor, DelegationAuthority, DelegationCommand, DelegationOperation,
            DelegationReason, DeliveryCancellation,
        },
        internal_note::{
            AddInternalNote, AskOwnerOutcome, AskOwnerToAct, InternalNoteProvenance, StartAgentTask,
        },
        outreach::OutreachStatus,
        stuck_work::StuckWorkThresholds,
        task::{
            BackgroundTask, ChainStage, NewTask, ResumeActor, StopActor, TaskAttemptOutcome,
            TaskAttemptRecordStatus, TaskAttemptRef, TaskAttemptStatus, TaskBoardFilter,
            TaskChainCard, TaskChainCounts, TaskFailure, TaskLeaseRef, TaskOwner,
            TaskOwnershipActor, TaskOwnershipAuthority, TaskOwnershipCommand,
            TaskOwnershipOperation, TaskOwnershipReason, TaskSource, TaskStatus, TaskStatusEvent,
            TaskStopReason, TaskTransitionActorKind, TaskTransitionReason, ThreadActivity,
            TokenUsage,
        },
        transport::PrincipalId,
        value_objects::MessageId,
    },
    task_queue::{
        CreateOutreachRequest, DelegationCommandRequest, OutreachTargetIdentity,
        OutreachTargetRequest,
    },
    use_cases::thread::{AgentReply, MessageAuthorWrite, MessageWrite, qualified_email_identity},
};

#[tokio::test]
async fn competing_ask_agent_retries_requeue_once_and_fence_the_old_run() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let task = enqueue_chain(&persistence, company.id, channel.id, "note-instruction").await;
    let thread_id = task.thread_id.unwrap();
    let actor = PrincipalId::new(
        sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM principals WHERE company_id = $1 AND user_id = $2",
        )
        .bind(company.id)
        .bind(company.user_id)
        .fetch_one(&pool)
        .await
        .unwrap(),
    );
    let note = ThreadPersistence::create_internal_note(
        &persistence,
        &AddInternalNote {
            company_id: company.id,
            channel_id: channel.id,
            thread_id,
            text: "The replacement purchase order is PO-42.".into(),
            command_id: Uuid::new_v4(),
            supersedes_note_id: None,
            provenance: InternalNoteProvenance::Api,
        },
        actor,
    )
    .await
    .unwrap()
    .internal_note
    .unwrap();
    let other_task = enqueue_chain(&persistence, company.id, channel.id, "other-note-thread").await;
    let other_note = ThreadPersistence::create_internal_note(
        &persistence,
        &AddInternalNote {
            company_id: company.id,
            channel_id: channel.id,
            thread_id: other_task.thread_id.unwrap(),
            text: "This note belongs to a different thread.".into(),
            command_id: Uuid::new_v4(),
            supersedes_note_id: None,
            provenance: InternalNoteProvenance::Api,
        },
        actor,
    )
    .await
    .unwrap()
    .internal_note
    .unwrap();
    let wrong_thread = AskOwnerToAct {
        company_id: company.id,
        channel_id: channel.id,
        thread_id,
        task_id: task.id,
        expected_ownership_version: task.ownership.version,
        note_ids: vec![other_note.id],
        command_id: Uuid::new_v4(),
    };
    assert!(matches!(
        persistence.ask_owner_to_act(&wrong_thread, actor).await,
        Err(AppError::BadRequest(_))
    ));
    let cross_company = AskOwnerToAct {
        company_id: Uuid::new_v4(),
        note_ids: vec![note.id],
        command_id: Uuid::new_v4(),
        ..wrong_thread
    };
    assert!(matches!(
        persistence.ask_owner_to_act(&cross_company, actor).await,
        Err(AppError::NotFound(_))
    ));
    let old_lease = claim(&persistence, task.id).await;
    let processing = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
    let command = AskOwnerToAct {
        company_id: company.id,
        channel_id: channel.id,
        thread_id,
        task_id: task.id,
        expected_ownership_version: processing.ownership.version,
        note_ids: vec![note.id],
        command_id: Uuid::new_v4(),
    };
    let (first, retry) = tokio::join!(
        persistence.ask_owner_to_act(&command, actor),
        persistence.ask_owner_to_act(&command, actor),
    );
    assert_eq!(first.unwrap(), AskOwnerOutcome::Requeued);
    assert_eq!(retry.unwrap(), AskOwnerOutcome::Requeued);
    let pending = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
    assert_eq!(pending.status, TaskStatus::Pending);
    assert!(pending.worker_id.is_none());
    assert!(matches!(
        persistence
            .claim_agent_instruction_notes(company.id, thread_id, old_lease)
            .await,
        Err(AppError::Conflict(_))
    ));
    assert!(
        !persistence
            .renew_task_lease(old_lease, Utc::now() + chrono::Duration::minutes(5))
            .await
            .unwrap()
    );

    let new_lease = claim(&persistence, task.id).await;
    let selected = persistence
        .claim_agent_instruction_notes(company.id, thread_id, new_lease)
        .await
        .unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].note_id, note.id);
    assert!(selected[0].body.contains("PO-42"));
    assert!(
        persistence
            .claim_agent_instruction_notes(company.id, thread_id, new_lease)
            .await
            .unwrap()
            .is_empty(),
        "one execution receives each selected note only once"
    );
    assert!(persistence.mark_task_completed(new_lease).await.unwrap());
    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn start_agent_task_is_idempotent_and_delivers_selected_notes_to_its_first_run() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let completed = enqueue_chain(
        &persistence,
        company.id,
        channel.id,
        "completed-before-note",
    )
    .await;
    let thread_id = completed.thread_id.unwrap();
    let completed_lease = claim(&persistence, completed.id).await;
    assert!(
        persistence
            .mark_task_completed(completed_lease)
            .await
            .unwrap()
    );
    let actor = PrincipalId::new(
        sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM principals WHERE company_id = $1 AND user_id = $2",
        )
        .bind(company.id)
        .bind(company.user_id)
        .fetch_one(&pool)
        .await
        .unwrap(),
    );
    let note = ThreadPersistence::create_internal_note(
        &persistence,
        &AddInternalNote {
            company_id: company.id,
            channel_id: channel.id,
            thread_id,
            text: "Use contract revision 7, not revision 6.".into(),
            command_id: Uuid::new_v4(),
            supersedes_note_id: None,
            provenance: InternalNoteProvenance::Api,
        },
        actor,
    )
    .await
    .unwrap()
    .internal_note
    .unwrap();
    let command = StartAgentTask {
        company_id: company.id,
        channel_id: channel.id,
        thread_id,
        note_ids: vec![note.id],
        command_id: Uuid::new_v4(),
    };
    let started = persistence.start_agent_task(&command, actor).await.unwrap();
    let retry = persistence.start_agent_task(&command, actor).await.unwrap();
    assert_eq!(retry.id, started.id);
    assert_note_request_uses_platform_author(&pool, company.id).await;
    assert_eq!(started.status, TaskStatus::Pending);
    assert!(matches!(started.ownership.owner, TaskOwner::Agent(_)));

    let lease = claim(&persistence, started.id).await;
    let notes = persistence
        .claim_agent_instruction_notes(company.id, thread_id, lease)
        .await
        .unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].note_id, note.id);
    assert!(notes[0].body.contains("revision 7"));
    assert!(persistence.mark_task_completed(lease).await.unwrap());

    let unassigned_channel = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "No agent selected".into(),
            slug: format!("no-agent-{}", Uuid::new_v4().simple()),
            agent_ids: Some(Vec::new()),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let unassigned_thread = ThreadPersistence::create_thread(
        &persistence,
        unassigned_channel.id,
        "Notes awaiting an owner",
        &[],
    )
    .await
    .unwrap();
    let unassigned_note = ThreadPersistence::create_internal_note(
        &persistence,
        &AddInternalNote {
            company_id: company.id,
            channel_id: unassigned_channel.id,
            thread_id: unassigned_thread.id,
            text: "An operator must assign an agent before this can run.".into(),
            command_id: Uuid::new_v4(),
            supersedes_note_id: None,
            provenance: InternalNoteProvenance::Api,
        },
        actor,
    )
    .await
    .unwrap()
    .internal_note
    .unwrap();
    let unassigned = persistence
        .start_agent_task(
            &StartAgentTask {
                company_id: company.id,
                channel_id: unassigned_channel.id,
                thread_id: unassigned_thread.id,
                note_ids: vec![unassigned_note.id],
                command_id: Uuid::new_v4(),
            },
            actor,
        )
        .await
        .unwrap();
    assert_eq!(unassigned.status, TaskStatus::Pending);
    assert!(matches!(unassigned.ownership.owner, TaskOwner::Unassigned));
    assert!(
        !persistence
            .claim_task(
                unassigned.id,
                Uuid::new_v4(),
                Utc::now() + chrono::Duration::minutes(5),
            )
            .await
            .unwrap(),
        "unassigned work remains visible but cannot run under an invented owner"
    );

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// What a fixture that forces a status writes instead of an attribution. It has no cause to
/// state, and leaving the columns out would carry the previous transition's into the event.
///
/// The ledger row it produces falls to the trigger's deterministic mapping, and lands on
/// `unknown` for any status pair that mapping does not cover -- which is the honest record of
/// a fixture reaching into the table. Production callers all state their cause, so a scoped
/// lifecycle assertion still expects no `unknown` rows of its own.
const CLEAR_TRANSITION: &str = "transition_reason = NULL, transition_actor_kind = NULL, \
         transition_actor_id = NULL, transition_approval_id = NULL, transition_outreach_id = NULL";
use crate::entities::creation::CreationProvenance;
use crate::use_cases::{
    agent::{AgentPersistence, AgentWrite},
    channel::{ChannelPersistence, ChannelWrite},
    company::{CompanyPersistence, CompanyWrite},
    thread::ThreadPersistence,
    user::UserPersistence,
};

#[test]
fn quorum_threshold_rounds_up() {
    assert_eq!(required_response_count(1, 100.0), 1);
    assert_eq!(required_response_count(3, 50.0), 2);
    assert_eq!(required_response_count(4, 50.0), 2);
    assert_eq!(required_response_count(10, 20.0), 2);
}

fn ownership_command(
    task: &BackgroundTask,
    actor: PrincipalId,
    authority: TaskOwnershipAuthority,
    operation: TaskOwnershipOperation,
    new_owner: TaskOwner,
) -> TaskOwnershipCommand {
    TaskOwnershipCommand {
        execution: None,
        invocation: None,
        task_id: task.id,
        company_id: task.company_id,
        command_id: Uuid::new_v4(),
        expected_version: task.ownership.version,
        actor: TaskOwnershipActor {
            principal_id: actor,
            authority,
        },
        operation,
        new_owner,
        reason: match operation {
            TaskOwnershipOperation::Claim => TaskOwnershipReason::SelfClaim,
            TaskOwnershipOperation::Assign => TaskOwnershipReason::ManualAssignment,
            TaskOwnershipOperation::Transfer => TaskOwnershipReason::Delegated,
            TaskOwnershipOperation::Release => TaskOwnershipReason::Released,
            TaskOwnershipOperation::InitialAssignment | TaskOwnershipOperation::OwnerRemoved => {
                TaskOwnershipReason::OwnerRemoved
            }
        },
        reason_detail: None,
        handoff_instruction: (operation == TaskOwnershipOperation::Transfer)
            .then(|| "Continue from the existing thread and answer the open request.".into()),
    }
}

#[tokio::test]
async fn competing_ownership_claims_are_versioned_and_idempotent() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let task = enqueue_chain(&persistence, company.id, channel.id, "ownership-claims").await;
    assert!(matches!(task.ownership.owner, TaskOwner::Agent(_)));

    let manager_id = PrincipalId::new(
        sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM principals WHERE company_id = $1 AND user_id = $2",
        )
        .bind(company.id)
        .bind(company.user_id)
        .fetch_one(&pool)
        .await
        .unwrap(),
    );
    let released = persistence
        .change_task_ownership(ownership_command(
            &task,
            manager_id,
            TaskOwnershipAuthority::Manager,
            TaskOwnershipOperation::Release,
            TaskOwner::Unassigned,
        ))
        .await
        .unwrap();
    assert_eq!(released.to_version, 2);

    let suffix = Uuid::new_v4().simple().to_string();
    let member_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users (id, username, email, password_hash) VALUES ($1, $2, $3, 'hash')",
    )
    .bind(member_id)
    .bind(format!("claimant-{suffix}"))
    .bind(format!("claimant-{suffix}@example.test"))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO company_members (id, company_id, user_id, role) VALUES ($1, $2, $3, 'member')",
    )
    .bind(Uuid::new_v4())
    .bind(company.id)
    .bind(member_id)
    .execute(&pool)
    .await
    .unwrap();
    let member_principal = PrincipalId::random();
    sqlx::query(
        "INSERT INTO principals (id, company_id, kind, user_id, display_label) \
         VALUES ($1, $2, 'person', $3, 'Claimant')",
    )
    .bind(member_principal.as_uuid())
    .bind(company.id)
    .bind(member_id)
    .execute(&pool)
    .await
    .unwrap();

    let unassigned = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
    let first = ownership_command(
        &unassigned,
        manager_id,
        TaskOwnershipAuthority::UnassignedClaimant,
        TaskOwnershipOperation::Claim,
        TaskOwner::Human(manager_id),
    );
    let second = ownership_command(
        &unassigned,
        member_principal,
        TaskOwnershipAuthority::UnassignedClaimant,
        TaskOwnershipOperation::Claim,
        TaskOwner::Human(member_principal),
    );
    let (first_result, second_result) = tokio::join!(
        persistence.change_task_ownership(first.clone()),
        persistence.change_task_ownership(second.clone())
    );
    assert_eq!(
        usize::from(first_result.is_ok()) + usize::from(second_result.is_ok()),
        1
    );

    let (winning_command, event) = match (first_result, second_result) {
        (Ok(event), Err(AppError::Conflict(_))) => (first, event),
        (Err(AppError::Conflict(_)), Ok(event)) => (second, event),
        outcomes => panic!("one claimant should win and one should conflict: {outcomes:?}"),
    };
    let replay = persistence
        .change_task_ownership(winning_command.clone())
        .await
        .unwrap();
    assert_eq!(replay.id, event.id, "a retry returns its original event");
    let mut mismatched = winning_command;
    mismatched.reason_detail = Some("different payload".into());
    assert!(matches!(
        persistence.change_task_ownership(mismatched).await,
        Err(AppError::Conflict(_))
    ));

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn agent_transfer_revokes_the_old_execution_and_preserves_private_handoff() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let task = enqueue_chain(&persistence, company.id, channel.id, "ownership-transfer").await;
    let lease = claim(&persistence, task.id).await;

    let second_agent = AgentPersistence::create(
        &persistence,
        company.id,
        AgentWrite {
            name: "Second Agent".into(),
            slug: format!("second-agent-{}", Uuid::new_v4().simple()),
            created_by: Some(CreationProvenance::system()),
            harness_kind: Some(crate::entities::harness::HarnessKind::AiAgents),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO channel_agents (company_id, channel_id, agent_id, position) \
         VALUES ($1, $2, $3, 1)",
    )
    .bind(company.id)
    .bind(channel.id)
    .bind(second_agent.id)
    .execute(&pool)
    .await
    .unwrap();
    let second_principal = PrincipalId::new(
        sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM principals WHERE company_id = $1 AND agent_id = $2",
        )
        .bind(company.id)
        .bind(second_agent.id)
        .fetch_one(&pool)
        .await
        .unwrap(),
    );
    let manager = PrincipalId::new(
        sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM principals WHERE company_id = $1 AND user_id = $2",
        )
        .bind(company.id)
        .bind(company.user_id)
        .fetch_one(&pool)
        .await
        .unwrap(),
    );

    let processing = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
    let command = ownership_command(
        &processing,
        manager,
        TaskOwnershipAuthority::Manager,
        TaskOwnershipOperation::Transfer,
        TaskOwner::Agent(second_principal),
    );
    let event = persistence.change_task_ownership(command).await.unwrap();
    assert_eq!(
        event.handoff_instruction.as_deref(),
        Some("Continue from the existing thread and answer the open request.")
    );

    let transferred = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
    assert_eq!(transferred.status, TaskStatus::Pending);
    assert_eq!(
        transferred.ownership.owner,
        TaskOwner::Agent(second_principal)
    );
    assert!(transferred.worker_id.is_none());
    assert!(
        !persistence
            .renew_task_lease(lease, Utc::now() + chrono::Duration::minutes(5))
            .await
            .unwrap()
    );
    assert!(
        persistence
            .owned_agent_execution(company.id, channel.id, lease)
            .await
            .unwrap()
            .is_none(),
        "the old owner cannot resolve an execution after transfer"
    );
    assert!(
        !persistence.mark_task_completed(lease).await.unwrap(),
        "the stale owner cannot close the task"
    );
    assert!(
        !persistence
            .mark_task_failed(TaskFailure {
                lease,
                error: "stale owner",
                next_run_at: Utc::now(),
                outcome: TaskFailureOutcome::Retry,
                reason: TaskStopReason::RetryableFailure,
            })
            .await
            .unwrap(),
        "the stale owner cannot record a failure"
    );
    assert!(
        sqlx::query("UPDATE task_ownership_events SET reason_detail = 'tampered' WHERE id = $1")
            .bind(event.id)
            .execute(&pool)
            .await
            .is_err()
    );

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn human_completion_commits_one_reply_delivery_and_terminal_transition() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let task = enqueue_chain(&persistence, company.id, channel.id, "human-completion").await;
    let owner = PrincipalId::new(
        sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM principals WHERE company_id = $1 AND user_id = $2",
        )
        .bind(company.id)
        .bind(company.user_id)
        .fetch_one(&pool)
        .await
        .unwrap(),
    );
    let transferred = persistence
        .change_task_ownership(ownership_command(
            &task,
            owner,
            TaskOwnershipAuthority::Manager,
            TaskOwnershipOperation::Transfer,
            TaskOwner::Human(owner),
        ))
        .await
        .unwrap();
    let owned = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
    let thread_id = owned.thread_id.unwrap();
    let message = MessageWrite::internal(
        thread_id,
        MessageAuthorWrite::Principal(owner),
        "Re: human completion",
        "The final answer.",
        MessageDirection::Outbound,
        MessageRole::Human,
        owned.correlation_id,
    )
    .external_conversation();
    let mut delivery = delivery_fixture(
        &persistence,
        DeliveryFixtureRequest {
            task_id: Some(task.id),
            ..DeliveryFixtureRequest::new(company.id, channel.id, thread_id, "human-completion")
        },
    )
    .await
    .delivery;
    delivery.message_id = message.id;
    delivery.correlation_id = owned.correlation_id;

    let command_id = Uuid::new_v4();
    let complete = |draft_version| HumanTaskCompletion {
        task_id: task.id,
        company_id: company.id,
        owner_principal_id: owner,
        expected_ownership_version: transferred.to_version,
        command_id,
        command_fingerprint: "same-completion".into(),
        draft_id: crate::entities::response_draft::ResponseDraftId::new(command_id),
        draft_version,
        recipient_snapshot: crate::entities::response_draft::DraftRecipientSnapshot::email(
            crate::entities::value_objects::EmailAddress::from("customer@example.com"),
            Vec::new(),
        ),
        evidence: Vec::new(),
        message: &message,
        deliveries: vec![delivery.clone()],
    };
    assert!(matches!(
        persistence.complete_human_task(complete(2)).await,
        Err(AppError::Conflict(_))
    ));
    let first = persistence.complete_human_task(complete(1)).await.unwrap();
    let replay = persistence.complete_human_task(complete(1)).await.unwrap();
    assert_eq!(first.message_id, replay.message_id);
    assert_eq!(first.deliveries.len(), 1);
    assert!(replay.deliveries.is_empty());
    let draft: (String, i32, serde_json::Value) = sqlx::query_as(
        "SELECT status, version, recipient_snapshot FROM response_drafts WHERE id = $1",
    )
    .bind(command_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(draft.0, "published");
    assert_eq!(draft.1, 1);
    assert_eq!(draft.2["version"], "1");
    let publication_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM response_draft_publications WHERE draft_id = $1")
            .bind(command_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(publication_count, 1, "a retry reuses one publication");
    assert_eq!(
        persistence
            .get_task_by_id(task.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Completed
    );
    let delivery_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM message_deliveries WHERE task_id = $1 AND message_id = $2",
    )
    .bind(task.id)
    .bind(message.id.as_uuid())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(delivery_count, 1);

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

struct PendingResponseReview {
    company_id: Uuid,
    channel_id: Uuid,
    task_id: Uuid,
    thread_id: Uuid,
    owner: PrincipalId,
    draft_id: crate::entities::response_draft::ResponseDraftId,
    message: MessageWrite,
    evidence_hash: String,
}

async fn pending_response_review(
    persistence: &PostgresPersistence,
    pool: &sqlx::PgPool,
) -> PendingResponseReview {
    let (company, channel) = seed_company_and_channel(persistence).await;
    ResponseReviewPersistence::set_company_review_policy(
        persistence,
        company.id,
        ExternalResponseReview::ReviewAllExternal,
    )
    .await
    .unwrap();
    let task = enqueue_chain(persistence, company.id, channel.id, "response-review").await;
    let owner = PrincipalId::new(
        sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM principals WHERE company_id = $1 AND user_id = $2",
        )
        .bind(company.id)
        .bind(company.user_id)
        .fetch_one(pool)
        .await
        .unwrap(),
    );
    let transferred = persistence
        .change_task_ownership(ownership_command(
            &task,
            owner,
            TaskOwnershipAuthority::Manager,
            TaskOwnershipOperation::Transfer,
            TaskOwner::Human(owner),
        ))
        .await
        .unwrap();
    let owned = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
    let thread_id = owned.thread_id.unwrap();
    let removed_attachment_hash = format!("removed-{}", Uuid::new_v4());
    let mut evidence_message = MessageWrite::internal(
        thread_id,
        MessageAuthorWrite::Principal(owner),
        "Private source",
        "The attachment bytes are no longer available.",
        MessageDirection::Inbound,
        MessageRole::Human,
        owned.correlation_id,
    );
    evidence_message
        .attachments
        .push(crate::entities::message::AttachmentMetadata {
            filename: "private-source.txt".into(),
            content_type: "text/plain".into(),
            sha256_hash: removed_attachment_hash.clone(),
            size_bytes: 32,
            storage_key: None,
            source: None,
        });
    ThreadPersistence::create_message(persistence, &evidence_message)
        .await
        .unwrap();
    let message = MessageWrite::internal(
        thread_id,
        MessageAuthorWrite::Principal(owner),
        "Re: review",
        "Exact reviewed answer.",
        MessageDirection::Outbound,
        MessageRole::Human,
        owned.correlation_id,
    )
    .external_conversation();
    let mut delivery = delivery_fixture(
        persistence,
        DeliveryFixtureRequest {
            task_id: Some(task.id),
            body: "Exact reviewed answer.",
            ..DeliveryFixtureRequest::new(company.id, channel.id, thread_id, "response-review")
        },
    )
    .await
    .delivery;
    delivery.message_id = message.id;
    delivery.correlation_id = owned.correlation_id;
    let draft_id = crate::entities::response_draft::ResponseDraftId::random();
    let result = persistence
        .complete_human_task(HumanTaskCompletion {
            task_id: task.id,
            company_id: company.id,
            owner_principal_id: owner,
            expected_ownership_version: transferred.to_version,
            command_id: Uuid::new_v4(),
            command_fingerprint: "review-fixture".into(),
            draft_id,
            draft_version: 1,
            recipient_snapshot: crate::entities::response_draft::DraftRecipientSnapshot::email(
                "customer@example.com".into(),
                Vec::new(),
            ),
            evidence: vec![crate::entities::response_draft::ResponseEvidence {
                id: Uuid::new_v4(),
                source: crate::entities::response_draft::EvidenceSource::Attachment {
                    message_id: evidence_message.id,
                    sha256_hash: removed_attachment_hash.clone(),
                },
                source_version: "attachment-v1".into(),
                content_digest: "missing-object-digest".into(),
                audience: crate::entities::response_draft::EvidenceAudience::InternalOnly,
                support: crate::entities::response_draft::EvidenceSupport::DirectEvidence,
            }],
            message: &message,
            deliveries: vec![delivery],
        })
        .await
        .unwrap();
    assert!(result.pending_review);
    assert!(result.deliveries.is_empty());
    PendingResponseReview {
        company_id: company.id,
        channel_id: channel.id,
        task_id: task.id,
        thread_id,
        owner,
        draft_id,
        message,
        evidence_hash: removed_attachment_hash,
    }
}

#[tokio::test]
async fn response_review_approval_is_exact_idempotent_and_serialized() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let fixture = pending_response_review(&persistence, &pool).await;
    assert_eq!(
        persistence
            .get_task_by_id(fixture.task_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::PendingApproval
    );
    let message_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE id = $1")
        .bind(fixture.message.id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        message_count, 0,
        "review must not expose a canonical message"
    );
    let detail = persistence
        .get_for_reviewer(fixture.company_id, fixture.draft_id, fixture.owner)
        .await
        .unwrap()
        .unwrap();
    assert!(!detail.evidence[0].openable);
    let unavailable_reason = detail.evidence[0].unavailable_reason.as_deref().unwrap();
    assert!(!unavailable_reason.contains(&fixture.evidence_hash));
    assert!(!unavailable_reason.contains("attachment"));
    assert!(
        sqlx::query(
            "UPDATE response_draft_evidence SET content_digest = 'tampered' WHERE draft_id = $1"
        )
        .bind(fixture.draft_id.as_uuid())
        .execute(&pool)
        .await
        .is_err(),
        "a retained evidence set is immutable once its review row exists"
    );
    assert!(
        detail.draft.source_handoff_generation.is_some(),
        "a draft created after manual transfer retains the handoff event identity"
    );
    let publication = persistence
        .publication_for_reviewer(fixture.company_id, fixture.draft_id, 1, fixture.owner)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !serde_json::to_string(&publication)
            .unwrap()
            .contains("Continue from the existing thread"),
        "private handoff content must never enter the publication snapshot"
    );

    let command = |command_id| ReviewCommand {
        company_id: fixture.company_id,
        draft_id: fixture.draft_id,
        expected_draft_version: 1,
        command_id,
        actor_principal_id: fixture.owner,
        action: ReviewAction::Approve {
            rationale: Some("Verified against the request.".into()),
        },
    };
    assert!(matches!(
        persistence
            .execute_review_command(ReviewCommand {
                company_id: Uuid::new_v4(),
                ..command(Uuid::new_v4())
            })
            .await,
        Err(AppError::Conflict(_))
    ));
    let first_id = Uuid::new_v4();
    let second_id = Uuid::new_v4();
    let (first, second) = tokio::join!(
        persistence.execute_review_command(command(first_id)),
        persistence.execute_review_command(command(second_id)),
    );
    let (winner_id, published) = match (first, second) {
        (Ok(result), Err(AppError::Conflict(_))) => (first_id, result),
        (Err(AppError::Conflict(_)), Ok(result)) => (second_id, result),
        other => panic!("exactly one reviewer command must publish: {other:?}"),
    };
    let replay = persistence
        .execute_review_command(command(winner_id))
        .await
        .unwrap();
    assert_eq!(
        replay, published,
        "a command retry returns its original result"
    );
    assert_eq!(published.published_message_id, Some(fixture.message.id));
    let counts: (i64, i64, i64) = sqlx::query_as(
        r#"SELECT
             (SELECT COUNT(*) FROM messages WHERE id = $1),
             (SELECT COUNT(*) FROM message_deliveries WHERE task_id = $2),
             (SELECT COUNT(*) FROM response_draft_publications WHERE draft_id = $3)"#,
    )
    .bind(fixture.message.id.as_uuid())
    .bind(fixture.task_id)
    .bind(fixture.draft_id.as_uuid())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(counts, (1, 1, 1));
    assert_eq!(
        persistence
            .get_task_by_id(fixture.task_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Completed
    );
    CompanyPersistence::delete(&persistence, fixture.company_id)
        .await
        .unwrap();
}

#[tokio::test]
async fn concurrent_retries_of_one_review_command_return_one_original_result() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let fixture = pending_response_review(&persistence, &pool).await;
    let command = ReviewCommand {
        company_id: fixture.company_id,
        draft_id: fixture.draft_id,
        expected_draft_version: 1,
        command_id: Uuid::new_v4(),
        actor_principal_id: fixture.owner,
        action: ReviewAction::Approve { rationale: None },
    };
    let (first, retry) = tokio::join!(
        persistence.execute_review_command(command.clone()),
        persistence.execute_review_command(command),
    );
    assert_eq!(first.unwrap(), retry.unwrap());
    let publication_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM response_draft_publications WHERE draft_id = $1")
            .bind(fixture.draft_id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(publication_count, 1);
    CompanyPersistence::delete(&persistence, fixture.company_id)
        .await
        .unwrap();
}

#[tokio::test]
async fn editing_recipients_creates_a_new_version_and_stales_the_old_approval() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let fixture = pending_response_review(&persistence, &pool).await;
    let current = persistence
        .publication_for_reviewer(fixture.company_id, fixture.draft_id, 1, fixture.owner)
        .await
        .unwrap()
        .unwrap();
    let queued = delivery_fixture(
        &persistence,
        DeliveryFixtureRequest {
            task_id: Some(fixture.task_id),
            recipient: "corrected@example.com",
            subject: "Re: corrected",
            body: "Edited exact answer.",
            ..DeliveryFixtureRequest::new(
                fixture.company_id,
                fixture.channel_id,
                fixture.thread_id,
                "response-review-edit",
            )
        },
    )
    .await;
    sqlx::query("DELETE FROM messages WHERE id = $1")
        .bind(queued.message_id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let mut edited_message = current.message().clone();
    edited_message.id = queued.delivery.message_id;
    edited_message.subject = "Re: corrected".into();
    edited_message.clean_text_body = "Edited exact answer.".into();
    let publication =
        DraftPublicationSnapshot::new(edited_message.clone(), queued.delivery).unwrap();
    let replacement = PreparedReviewDraft::new(
        fixture.draft_id,
        2,
        fixture.company_id,
        fixture.channel_id,
        fixture.thread_id,
        Some(fixture.task_id),
        fixture.owner,
        fixture.owner,
        crate::entities::response_draft::DraftRecipientSnapshot::email(
            "corrected@example.com".into(),
            Vec::new(),
        ),
        Vec::new(),
        publication,
    )
    .unwrap();
    persistence
        .execute_review_command(ReviewCommand {
            company_id: fixture.company_id,
            draft_id: fixture.draft_id,
            expected_draft_version: 1,
            command_id: Uuid::new_v4(),
            actor_principal_id: fixture.owner,
            action: ReviewAction::Edit {
                replacement: Box::new(replacement),
            },
        })
        .await
        .unwrap();
    assert!(matches!(
        persistence
            .execute_review_command(ReviewCommand {
                company_id: fixture.company_id,
                draft_id: fixture.draft_id,
                expected_draft_version: 1,
                command_id: Uuid::new_v4(),
                actor_principal_id: fixture.owner,
                action: ReviewAction::Approve { rationale: None },
            })
            .await,
        Err(AppError::Conflict(_))
    ));
    let detail = persistence
        .get_for_reviewer(fixture.company_id, fixture.draft_id, fixture.owner)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(detail.draft.version, 2);
    assert_eq!(detail.history[0].status.as_str(), "superseded");
    assert_eq!(
        detail.draft.recipients,
        crate::entities::response_draft::DraftRecipientSnapshot::email(
            "corrected@example.com".into(),
            Vec::new()
        )
    );
    let published = persistence
        .execute_review_command(ReviewCommand {
            company_id: fixture.company_id,
            draft_id: fixture.draft_id,
            expected_draft_version: 2,
            command_id: Uuid::new_v4(),
            actor_principal_id: fixture.owner,
            action: ReviewAction::Approve { rationale: None },
        })
        .await
        .unwrap();
    assert_eq!(published.published_message_id, Some(edited_message.id));
    let old_message_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE id = $1")
        .bind(fixture.message.id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(old_message_count, 0);
    CompanyPersistence::delete(&persistence, fixture.company_id)
        .await
        .unwrap();
}

#[tokio::test]
async fn review_policy_parks_agent_dispatch_and_private_rejection_feedback_reaches_retry() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let (company, channel) = seed_company_and_channel(&persistence).await;
    ResponseReviewPersistence::set_company_review_policy(
        &persistence,
        company.id,
        ExternalResponseReview::ReviewAllExternal,
    )
    .await
    .unwrap();
    let task = enqueue_chain(&persistence, company.id, channel.id, "review-agent").await;
    let lease = claim(&persistence, task.id).await;
    let agent_principal = lease.claimed_owner.agent_principal_id().unwrap();
    let agent_id: Uuid =
        sqlx::query_scalar("SELECT agent_id FROM principals WHERE company_id = $1 AND id = $2")
            .bind(company.id)
            .bind(agent_principal.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    let thread_id = task.thread_id.unwrap();
    let reply = AgentReply {
        message: MessageWrite::internal(
            thread_id,
            MessageAuthorWrite::Agent(crate::use_cases::thread::AgentAuthor {
                agent_id,
                display_label: "Chain Agent".into(),
            }),
            "Re: review agent",
            "Agent answer awaiting review.",
            MessageDirection::Outbound,
            MessageRole::Agent,
            task.correlation_id,
        )
        .external_conversation(),
        also_in_threads: Vec::new(),
    };
    let mut delivery = delivery_fixture(
        &persistence,
        DeliveryFixtureRequest {
            task_id: Some(task.id),
            body: "Agent answer awaiting review.",
            ..DeliveryFixtureRequest::new(company.id, channel.id, thread_id, "review-agent")
        },
    )
    .await
    .delivery;
    delivery.message_id = reply.message.id;
    delivery.correlation_id = task.correlation_id;
    let outcome = persistence
        .commit_agent_dispatch(AgentDispatchCommit {
            lease,
            reply: &reply,
            deliveries: vec![delivery],
            review_candidate: Some(AgentReviewCandidate {
                recipients: crate::entities::response_draft::DraftRecipientSnapshot::email(
                    "customer@example.com".into(),
                    Vec::new(),
                ),
                evidence: Vec::new(),
            }),
            payload: serde_json::json!({"answer": "retained"}),
            complete_outreach: false,
        })
        .await
        .unwrap();
    let DispatchCommit::PendingReview {
        draft_id,
        draft_version,
    } = outcome
    else {
        panic!("agent response was not parked for review")
    };
    assert_eq!(draft_version, 1);
    assert_eq!(
        persistence
            .get_task_by_id(task.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::PendingApproval
    );
    let counts: (i64, i64) = sqlx::query_as(
        r#"SELECT
             (SELECT COUNT(*) FROM messages WHERE id = $1),
             (SELECT COUNT(*) FROM message_deliveries WHERE task_id = $2)"#,
    )
    .bind(reply.message.id.as_uuid())
    .bind(task.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(counts, (0, 0));
    let owner = PrincipalId::new(
        sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM principals WHERE company_id = $1 AND user_id = $2",
        )
        .bind(company.id)
        .bind(company.user_id)
        .fetch_one(&pool)
        .await
        .unwrap(),
    );
    persistence
        .execute_review_command(ReviewCommand {
            company_id: company.id,
            draft_id,
            expected_draft_version: 1,
            command_id: Uuid::new_v4(),
            actor_principal_id: owner,
            action: ReviewAction::Reject {
                feedback: "Remove the unsupported delivery promise.".into(),
            },
        })
        .await
        .unwrap();
    assert_eq!(
        persistence
            .get_task_by_id(task.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Pending
    );
    let retry_lease = claim(&persistence, task.id).await;
    let retry = persistence
        .owned_agent_execution(company.id, channel.id, retry_lease)
        .await
        .unwrap()
        .expect("rejected agent task remains executable by its owner");
    assert_eq!(
        retry.review_feedback.as_deref(),
        Some("Remove the unsupported delivery promise.")
    );
    assert!(persistence.mark_task_completed(retry_lease).await.unwrap());
    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn review_reassignment_changes_responsibility_not_task_ownership_and_rejection_requeues() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let fixture = pending_response_review(&persistence, &pool).await;
    let suffix = Uuid::new_v4().simple().to_string();
    let username = format!("reviewer_{suffix}");
    let email = format!("{username}@example.com");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .unwrap();
    let reviewer_user = UserPersistence::get_by_email(&persistence, &email)
        .await
        .unwrap()
        .unwrap();
    sqlx::query(
        "INSERT INTO company_members (id, company_id, user_id, role) VALUES ($1, $2, $3, 'member')",
    )
    .bind(Uuid::new_v4())
    .bind(fixture.company_id)
    .bind(reviewer_user.id)
    .execute(&pool)
    .await
    .unwrap();
    let reviewer = PrincipalId::random();
    sqlx::query(
        r#"INSERT INTO principals (id, company_id, kind, user_id, display_label)
           VALUES ($1, $2, 'person', $3, 'Response Reviewer')"#,
    )
    .bind(reviewer.as_uuid())
    .bind(fixture.company_id)
    .bind(reviewer_user.id)
    .execute(&pool)
    .await
    .unwrap();
    persistence
        .execute_review_command(ReviewCommand {
            company_id: fixture.company_id,
            draft_id: fixture.draft_id,
            expected_draft_version: 1,
            command_id: Uuid::new_v4(),
            actor_principal_id: fixture.owner,
            action: ReviewAction::Reassign {
                reviewer_principal_id: reviewer,
            },
        })
        .await
        .unwrap();
    assert!(
        persistence
            .get_for_reviewer(fixture.company_id, fixture.draft_id, fixture.owner)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        persistence
            .get_for_reviewer(fixture.company_id, fixture.draft_id, reviewer)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        persistence
            .get_task_by_id(fixture.task_id)
            .await
            .unwrap()
            .unwrap()
            .ownership
            .owner,
        TaskOwner::Human(fixture.owner)
    );
    persistence
        .execute_review_command(ReviewCommand {
            company_id: fixture.company_id,
            draft_id: fixture.draft_id,
            expected_draft_version: 1,
            command_id: Uuid::new_v4(),
            actor_principal_id: reviewer,
            action: ReviewAction::Reject {
                feedback: "Correct the recipient before sending.".into(),
            },
        })
        .await
        .unwrap();
    assert_eq!(
        persistence
            .get_task_by_id(fixture.task_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Pending
    );
    let decision: (String, String) = sqlx::query_as(
        "SELECT status, feedback FROM response_reviews WHERE draft_id = $1 AND draft_version = 1",
    )
    .bind(fixture.draft_id.as_uuid())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        decision,
        (
            "rejected".into(),
            "Correct the recipient before sending.".into()
        )
    );
    sqlx::query("UPDATE channels SET access_mode = 'allowlist' WHERE company_id = $1 AND id = $2")
        .bind(fixture.company_id)
        .bind(fixture.channel_id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        persistence
            .get_for_reviewer(fixture.company_id, fixture.draft_id, reviewer)
            .await
            .unwrap()
            .is_none(),
        "an assigned reviewer who loses channel access must lose draft access too"
    );
    CompanyPersistence::delete(&persistence, fixture.company_id)
        .await
        .unwrap();
}

#[tokio::test]
async fn expired_review_cannot_publish_and_releases_the_task_for_regeneration() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let fixture = pending_response_review(&persistence, &pool).await;
    sqlx::query(
        r#"UPDATE response_reviews
           SET created_at = CURRENT_TIMESTAMP - INTERVAL '2 seconds',
               expires_at = CURRENT_TIMESTAMP - INTERVAL '1 second'
           WHERE draft_id = $1"#,
    )
    .bind(fixture.draft_id.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    let detail = persistence
        .get_for_reviewer(fixture.company_id, fixture.draft_id, fixture.owner)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(detail.review.status.as_str(), "expired");
    assert!(matches!(
        persistence
            .execute_review_command(ReviewCommand {
                company_id: fixture.company_id,
                draft_id: fixture.draft_id,
                expected_draft_version: 1,
                command_id: Uuid::new_v4(),
                actor_principal_id: fixture.owner,
                action: ReviewAction::Approve { rationale: None },
            })
            .await,
        Err(AppError::Conflict(_))
    ));
    assert_eq!(
        persistence
            .get_task_by_id(fixture.task_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Pending
    );
    let message_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE id = $1")
        .bind(fixture.message.id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(message_count, 0);
    CompanyPersistence::delete(&persistence, fixture.company_id)
        .await
        .unwrap();
}

#[tokio::test]
async fn response_review_policy_round_trips_company_override_and_preferred_reviewer() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let owner = PrincipalId::new(
        sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM principals WHERE company_id = $1 AND user_id = $2",
        )
        .bind(company.id)
        .bind(company.user_id)
        .fetch_one(&pool)
        .await
        .unwrap(),
    );
    let initial = persistence
        .review_policy(company.id, channel.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(initial.effective, ExternalResponseReview::Autonomous);
    persistence
        .set_company_review_policy(company.id, ExternalResponseReview::ReviewAllExternal)
        .await
        .unwrap();
    persistence
        .set_channel_review_policy(
            company.id,
            channel.id,
            Some(ExternalResponseReview::Autonomous),
            Some(owner),
        )
        .await
        .unwrap();
    let overridden = persistence
        .review_policy(company.id, channel.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        overridden.company_default,
        ExternalResponseReview::ReviewAllExternal
    );
    assert_eq!(
        overridden.channel_override,
        Some(ExternalResponseReview::Autonomous)
    );
    assert_eq!(overridden.preferred_reviewer_principal_id, Some(owner));
    persistence
        .set_channel_review_policy(company.id, channel.id, None, None)
        .await
        .unwrap();
    let inherited = persistence
        .review_policy(company.id, channel.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(inherited.channel_override, None);
    assert_eq!(
        inherited.effective,
        ExternalResponseReview::ReviewAllExternal
    );
    assert_eq!(inherited.preferred_reviewer_principal_id, None);
    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn task_chain_board_groups_by_correlation_and_keeps_complete_chain_under_channel_filter() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let suffix = Uuid::new_v4().simple().to_string();
    let username = format!("board_owner_{suffix}");
    let email = format!("{username}@example.com");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(&persistence, &email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        &persistence,
        owner.id,
        CompanyWrite {
            name: "Board Test".into(),
            slug: format!("board-test-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let first_channel = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "First Channel".into(),
            slug: "first".into(),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let second_channel = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "Second Channel".into(),
            slug: "second".into(),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let participant = crate::entities::value_objects::EmailAddress::from(email);
    let thread = persistence
        .create_thread(
            first_channel.id,
            "Root chain subject",
            std::slice::from_ref(&participant),
        )
        .await
        .unwrap();
    let root = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            first_channel.id,
            Some(thread.id),
            "root_task",
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    let nested = persistence
        .enqueue_task(NewTask {
            targets: Vec::new(),
            source: TaskSource::Unattributed,
            company_id: company.id,
            channel_id: second_channel.id,
            thread_id: None,
            task_type: "nested_task".into(),
            payload: serde_json::json!({}),
            correlation_id: root.correlation_id,
        })
        .await
        .unwrap();
    let unrelated = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            first_channel.id,
            Some(thread.id),
            "unrelated_same_thread",
            serde_json::json!({}),
        ))
        .await
        .unwrap();

    let filter = TaskBoardFilter::new(Some(second_channel.id), Utc::now());
    let board = persistence
        .list_task_chain_board(company.id, filter, &[second_channel.id])
        .await
        .unwrap();
    assert_eq!(board.total(ChainStage::Queued), 1);
    let card = &board.cards(ChainStage::Queued)[0];
    assert_eq!(card.correlation_id, root.correlation_id);
    assert_eq!(card.counts.total_tasks, 2);
    assert_eq!(card.title, "nested_task");
    assert_eq!(card.channel_names, vec!["Second Channel".to_string()]);

    let detail = persistence
        .get_task_chain_detail(
            CollaborationReadScope {
                company_id: company.id,
                visible_channel_ids: &[first_channel.id, second_channel.id],
            },
            root.correlation_id,
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        detail
            .tasks
            .iter()
            .map(|item| item.task.id)
            .collect::<Vec<_>>(),
        vec![root.id, nested.id]
    );
    assert_eq!(detail.events.len(), 2);
    assert!(
        detail
            .events
            .iter()
            .all(|event| event.reason == TaskTransitionReason::Enqueued)
    );
    let scoped = persistence
        .get_task_chain_detail(
            CollaborationReadScope {
                company_id: company.id,
                visible_channel_ids: &[second_channel.id],
            },
            root.correlation_id,
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(scoped.title, "nested_task");
    assert_eq!(scoped.channel_names, vec!["Second Channel"]);
    assert_eq!(scoped.tasks.len(), 1);
    assert_eq!(scoped.tasks[0].task.id, nested.id);
    assert!(scoped.events.iter().all(|event| event.task_id == nested.id));
    assert!(
        persistence
            .get_task_chain_detail(
                CollaborationReadScope {
                    company_id: Uuid::new_v4(),
                    visible_channel_ids: &[first_channel.id, second_channel.id],
                },
                root.correlation_id,
            )
            .await
            .unwrap()
            .is_none()
    );
    assert_ne!(unrelated.correlation_id, root.correlation_id);

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn status_event_constraints_reject_cross_company_rows_and_duplicate_sequences() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let suffix = Uuid::new_v4().simple().to_string();
    let username = format!("event_owner_{suffix}");
    let email = format!("{username}@example.com");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(&persistence, &email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        &persistence,
        owner.id,
        CompanyWrite {
            name: "Event Test".into(),
            slug: format!("event-test-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let channel = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "Events".into(),
            slug: "events".into(),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let task = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            channel.id,
            None,
            "event_test",
            serde_json::json!({}),
        ))
        .await
        .unwrap();

    let insert = |company_id: Uuid, sequence: i32| {
        sqlx::query(
            r#"INSERT INTO task_status_events (
                       id, company_id, task_id, correlation_id, sequence, from_status, to_status,
                       reason, actor_kind, retry_count, run_at, transitioned_at
                   ) VALUES ($1, $2, $3, $4, $5, 'pending', 'pending',
                             'operator_resumed', 'operator', 0, CURRENT_TIMESTAMP,
                             CURRENT_TIMESTAMP)"#,
        )
        .bind(Uuid::new_v4())
        .bind(company_id)
        .bind(task.id)
        .bind(task.correlation_id.as_uuid())
        .bind(sequence)
        .execute(&pool)
    };
    assert!(insert(Uuid::new_v4(), 2).await.is_err());
    assert!(insert(company.id, 1).await.is_err());

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn guarded_transitions_emit_only_on_success_and_operator_actions_record_the_user() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let suffix = Uuid::new_v4().simple().to_string();
    let username = format!("actor_owner_{suffix}");
    let email = format!("{username}@example.com");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(&persistence, &email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        &persistence,
        owner.id,
        CompanyWrite {
            name: "Actor Test".into(),
            slug: format!("actor-test-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let agent_id = seed_channel_agent(&persistence, company.id, "actors").await;
    let channel = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "Actors".into(),
            slug: "actors".into(),
            agent_ids: Some(vec![agent_id]),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let task = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            channel.id,
            None,
            "actor_test",
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    let worker = Uuid::new_v4();
    let expires = Utc::now() + chrono::Duration::minutes(5);
    assert!(
        persistence
            .claim_task(task.id, worker, expires)
            .await
            .unwrap()
    );
    assert!(
        !persistence
            .claim_task(task.id, Uuid::new_v4(), expires)
            .await
            .unwrap()
    );
    let before_stop = persistence
        .list_task_status_events(company.id, task.correlation_id, None, 20)
        .await
        .unwrap();
    assert_eq!(before_stop.len(), 2, "the failed guard must emit no event");

    persistence
        .stop_task(task.id, StopActor::Operator(owner.id))
        .await
        .unwrap();
    persistence
        .resume_task(task.id, ResumeActor::Operator(owner.id))
        .await
        .unwrap();
    let events = persistence
        .list_task_status_events(company.id, task.correlation_id, None, 20)
        .await
        .unwrap();
    for reason in [
        TaskTransitionReason::OperatorStopped,
        TaskTransitionReason::OperatorResumed,
    ] {
        let event = events.iter().find(|event| event.reason == reason).unwrap();
        assert_eq!(event.actor_kind, TaskTransitionActorKind::Operator);
        assert_eq!(event.actor_id, Some(owner.id));
    }

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// Row-local attribution is a write contract: every status-changing statement states all five
/// columns. The failure it guards against is silent -- a statement that omits one carries the
/// previous transition's value into the new row version, and the ledger records an actor that
/// had nothing to do with the change. This walks a task through the actor kinds in the order
/// most likely to expose that, and checks the boundaries where the kind changes.
#[tokio::test]
async fn consecutive_transitions_never_inherit_the_previous_actor_or_source() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let operator = Uuid::new_v4();
    let task = enqueue_chain(&persistence, company.id, channel.id, "inheritance").await;

    // worker -> approval.
    let first_worker = Uuid::new_v4();
    let lease = claim_as(&persistence, task.id, first_worker).await;
    let approval_id = park_for_approval(
        &persistence,
        &company,
        &channel,
        task.thread_id.unwrap(),
        lease,
    )
    .await;

    // approval -> worker: the claim must not carry the approval id that parked the task.
    persistence
        .resume_task(task.id, ResumeActor::Approval(approval_id))
        .await
        .unwrap();
    let second_worker = Uuid::new_v4();
    let lease = claim_as(&persistence, task.id, second_worker).await;

    // worker -> operator.
    persistence
        .mark_task_failed(TaskFailure {
            lease,
            error: "inheritance check",
            next_run_at: Utc::now(),
            outcome: TaskFailureOutcome::Retry,
            reason: TaskStopReason::RetryableFailure,
        })
        .await
        .unwrap();
    persistence
        .stop_task(task.id, StopActor::Operator(operator))
        .await
        .unwrap();

    let events = persistence
        .list_task_status_events(company.id, task.correlation_id, None, 50)
        .await
        .unwrap();
    let expected = [
        (
            TaskTransitionReason::Enqueued,
            TaskTransitionActorKind::System,
            None,
            None,
            None,
        ),
        (
            TaskTransitionReason::Claimed,
            TaskTransitionActorKind::Worker,
            Some(first_worker),
            None,
            None,
        ),
        (
            TaskTransitionReason::ApprovalRequested,
            TaskTransitionActorKind::Approval,
            None,
            Some(approval_id),
            None,
        ),
        (
            TaskTransitionReason::ApprovalAccepted,
            TaskTransitionActorKind::Approval,
            None,
            Some(approval_id),
            None,
        ),
        (
            TaskTransitionReason::Claimed,
            TaskTransitionActorKind::Worker,
            Some(second_worker),
            None,
            None,
        ),
        (
            TaskTransitionReason::RetryableFailure,
            TaskTransitionActorKind::Worker,
            Some(second_worker),
            None,
            None,
        ),
        (
            TaskTransitionReason::OperatorStopped,
            TaskTransitionActorKind::Operator,
            Some(operator),
            None,
            None,
        ),
    ];
    assert_eq!(events.len(), expected.len(), "{events:#?}");
    for (event, (reason, kind, actor_id, approval, outreach)) in events.iter().zip(expected) {
        assert_eq!(event.reason, reason, "{event:#?}");
        assert_eq!(event.actor_kind, kind, "{event:#?}");
        assert_eq!(event.actor_id, actor_id, "{event:#?}");
        assert_eq!(event.related_approval_id, approval, "{event:#?}");
        assert_eq!(event.related_outreach_id, outreach, "{event:#?}");
    }

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// Spend the whole retry budget and land the task in `dead_letter`, the way the worker does:
/// one failure per claim until the next one has nowhere left to go.
async fn exhaust_retry_budget(persistence: &PostgresPersistence, task: &BackgroundTask) {
    for attempt in 1..=task.max_retries {
        let lease = claim(persistence, task.id).await;
        let outcome = if attempt == task.max_retries {
            TaskFailureOutcome::DeadLetter
        } else {
            TaskFailureOutcome::Retry
        };
        assert!(
            persistence
                .mark_task_failed(TaskFailure {
                    lease,
                    error: "budget check",
                    next_run_at: Utc::now(),
                    outcome,
                    reason: TaskStopReason::RetryableFailure,
                })
                .await
                .unwrap()
        );
    }
    let exhausted = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
    assert_eq!(exhausted.status, TaskStatus::DeadLetter);
    assert!(
        exhausted.retry_count >= exhausted.max_retries,
        "the budget is spent: {exhausted:#?}"
    );
}

/// Every transition this chain made named its own cause. `unknown` is what the trigger records
/// when none did, so finding one here means a status write in the path under test is still
/// silent -- scoped to this chain, because the test database is shared and runs in parallel.
fn assert_no_unclassified_transitions(events: &[TaskStatusEvent]) {
    let unclassified: Vec<_> = events
        .iter()
        .filter(|event| event.reason == TaskTransitionReason::Unknown)
        .collect();
    assert!(
        unclassified.is_empty(),
        "transitions recorded no cause: {unclassified:#?}"
    );
}

/// Resume on a dead-lettered task has to hand back the retry budget, or it is theatre: the row
/// moves to `pending`, the worker claims it, the first failure computes
/// `retry_count + 1 >= max_retries` against a count that is already spent, and the task
/// dead-letters again having achieved nothing durable.
#[tokio::test]
async fn operator_resume_of_a_dead_lettered_task_restores_its_retry_budget() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let operator = Uuid::new_v4();
    let task = enqueue_chain(&persistence, company.id, channel.id, "dead_letter_resume").await;
    exhaust_retry_budget(&persistence, &task).await;

    let resumed = persistence
        .resume_task(task.id, ResumeActor::Operator(operator))
        .await
        .unwrap();

    assert_eq!(resumed.status, TaskStatus::Pending);
    assert_eq!(resumed.retry_count, 0, "the budget is fresh: {resumed:#?}");
    let events = persistence
        .list_task_status_events(company.id, task.correlation_id, None, 50)
        .await
        .unwrap();
    let resume_event = events
        .iter()
        .find(|event| event.reason == TaskTransitionReason::OperatorResumed)
        .expect("the resume is attributed to the operator, not guessed from the status pair");
    assert_eq!(resume_event.actor_kind, TaskTransitionActorKind::Operator);
    assert_eq!(resume_event.actor_id, Some(operator));
    assert_eq!(resume_event.from_status, Some(TaskStatus::DeadLetter));
    assert_no_unclassified_transitions(&events);

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// The Tasks page offers Resume on `stopped` as well as `dead_letter`, so an operator who stops
/// an exhausted task before resuming it reaches the same intent by a different route. Keying
/// the reset on the status alone would miss this one; keying it on the spent budget catches it.
#[tokio::test]
async fn operator_resume_of_a_stopped_exhausted_task_restores_its_retry_budget() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let operator = Uuid::new_v4();
    let task = enqueue_chain(&persistence, company.id, channel.id, "stopped_resume").await;
    exhaust_retry_budget(&persistence, &task).await;
    let stopped = persistence
        .stop_task(task.id, StopActor::Operator(operator))
        .await
        .unwrap();
    assert_eq!(stopped.status, TaskStatus::Stopped);
    assert!(stopped.retry_count >= stopped.max_retries);

    let resumed = persistence
        .resume_task(task.id, ResumeActor::Operator(operator))
        .await
        .unwrap();

    assert_eq!(resumed.status, TaskStatus::Pending);
    assert_eq!(resumed.retry_count, 0, "the budget is fresh: {resumed:#?}");
    let events = persistence
        .list_task_status_events(company.id, task.correlation_id, None, 50)
        .await
        .unwrap();
    assert_no_unclassified_transitions(&events);

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// An approval releasing a parked task is the same attempt carrying on, not a retry. Resetting
/// here would hand a task an unlimited budget for the price of one approval round trip.
#[tokio::test]
async fn approval_resume_continues_the_attempt_without_refunding_the_budget() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let task = enqueue_chain(&persistence, company.id, channel.id, "approval_resume").await;
    let lease = claim(&persistence, task.id).await;
    assert!(
        persistence
            .mark_task_failed(TaskFailure {
                lease,
                error: "one attempt already spent",
                next_run_at: Utc::now(),
                outcome: TaskFailureOutcome::Retry,
                reason: TaskStopReason::RetryableFailure,
            })
            .await
            .unwrap()
    );
    let lease = claim(&persistence, task.id).await;
    let approval_id = park_for_approval(
        &persistence,
        &company,
        &channel,
        task.thread_id.unwrap(),
        lease,
    )
    .await;
    let parked = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
    assert_eq!(parked.status, TaskStatus::PendingApproval);

    let resumed = persistence
        .resume_task(task.id, ResumeActor::Approval(approval_id))
        .await
        .unwrap();

    assert_eq!(resumed.status, TaskStatus::Pending);
    assert_eq!(
        resumed.retry_count, parked.retry_count,
        "a continuation spends nothing and refunds nothing: {resumed:#?}"
    );
    let events = persistence
        .list_task_status_events(company.id, task.correlation_id, None, 50)
        .await
        .unwrap();
    let resume_event = events
        .iter()
        .find(|event| event.reason == TaskTransitionReason::ApprovalAccepted)
        .expect("the approval is what released the task");
    assert_eq!(resume_event.actor_kind, TaskTransitionActorKind::Approval);
    assert_eq!(resume_event.related_approval_id, Some(approval_id));
    assert_no_unclassified_transitions(&events);

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// Each resume cause may only act on the states it can answer for. An approval must not
/// resurrect a task that was abandoned after exhausting its retries, and an operator must not
/// walk a task past the approval gate it is parked behind. Both mismatches match no row.
#[tokio::test]
async fn a_resume_cause_used_against_the_wrong_state_changes_nothing() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let operator = Uuid::new_v4();

    let abandoned = enqueue_chain(&persistence, company.id, channel.id, "wrong_state").await;
    exhaust_retry_budget(&persistence, &abandoned).await;
    assert!(
        persistence
            .resume_task(abandoned.id, ResumeActor::Approval(Uuid::new_v4()))
            .await
            .is_err(),
        "an approval cannot resume work it never parked"
    );
    let still_dead = persistence
        .get_task_by_id(abandoned.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(still_dead.status, TaskStatus::DeadLetter);

    let parked = enqueue_chain(&persistence, company.id, channel.id, "wrong_state").await;
    let lease = claim(&persistence, parked.id).await;
    park_for_approval(
        &persistence,
        &company,
        &channel,
        parked.thread_id.unwrap(),
        lease,
    )
    .await;
    assert!(
        persistence
            .resume_task(parked.id, ResumeActor::Operator(operator))
            .await
            .is_err(),
        "an operator resume must not bypass the approval the task is waiting on"
    );
    let still_parked = persistence
        .get_task_by_id(parked.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(still_parked.status, TaskStatus::PendingApproval);

    let events = persistence
        .list_task_status_events(company.id, parked.correlation_id, None, 50)
        .await
        .unwrap();
    assert!(
        !events
            .iter()
            .any(|event| event.reason == TaskTransitionReason::OperatorResumed),
        "a resume that matched no row writes no event: {events:#?}"
    );

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// The outreach -> operator boundary, which the sequence above does not reach: an outreach
/// names its own row as the transition's source, and the operator stop that follows must not
/// inherit it.
#[tokio::test]
async fn an_operator_stop_after_an_outreach_drops_the_outreach_source() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let operator = Uuid::new_v4();
    let task = enqueue_chain(&persistence, company.id, channel.id, "outreach-operator").await;
    let worker_id = Uuid::new_v4();
    let lease = claim_as(&persistence, task.id, worker_id).await;

    let outreach_id = Uuid::new_v4();
    let progress = persistence
        .create_outreach_and_pause(CreateOutreachRequest {
            invocation: None,
            correlation_id: task.correlation_id,
            id: outreach_id,
            lease,
            company_id: company.id,
            channel_id: channel.id,
            outreach_key: "attribution-outreach".into(),
            required_threshold_percent: 100.0,
            expires_at: Utc::now() + chrono::Duration::hours(24),
            subject: "Question".into(),
            body: "Please respond".into(),
            targets: Vec::new(),
        })
        .await
        .unwrap();
    assert!(progress.suspended);
    persistence
        .stop_task(task.id, StopActor::Operator(operator))
        .await
        .unwrap();

    let events = persistence
        .list_task_status_events(company.id, task.correlation_id, None, 50)
        .await
        .unwrap();
    let started = events
        .iter()
        .find(|event| event.reason == TaskTransitionReason::OutreachStarted)
        .expect("the outreach parked the task");
    assert_eq!(started.actor_kind, TaskTransitionActorKind::Outreach);
    assert_eq!(started.related_outreach_id, Some(outreach_id));
    assert_eq!(started.actor_id, None);

    let stopped = events
        .iter()
        .find(|event| event.reason == TaskTransitionReason::OperatorStopped)
        .expect("the operator stopped the task");
    assert_eq!(stopped.actor_kind, TaskTransitionActorKind::Operator);
    assert_eq!(stopped.actor_id, Some(operator));
    assert_eq!(
        stopped.related_outreach_id, None,
        "the operator stop must not inherit the outreach that parked the task"
    );

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// A rejected approval is an approval acting, and it may only end the task that approval
/// parked. Against any other state it must change nothing rather than reaching for work that
/// has since moved on.
#[tokio::test]
async fn an_approval_rejection_stops_only_the_task_it_parked() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let task = enqueue_chain(&persistence, company.id, channel.id, "approval-reject").await;
    let lease = claim_as(&persistence, task.id, Uuid::new_v4()).await;
    let approval_id = park_for_approval(
        &persistence,
        &company,
        &channel,
        task.thread_id.unwrap(),
        lease,
    )
    .await;

    persistence
        .stop_task(task.id, StopActor::Approval(approval_id))
        .await
        .unwrap();
    let events = persistence
        .list_task_status_events(company.id, task.correlation_id, None, 50)
        .await
        .unwrap();
    let rejected = events
        .iter()
        .find(|event| event.reason == TaskTransitionReason::ApprovalRejected)
        .expect("the rejection stopped the task");
    assert_eq!(rejected.actor_kind, TaskTransitionActorKind::Approval);
    assert_eq!(rejected.related_approval_id, Some(approval_id));
    assert_eq!(rejected.actor_id, None);

    // The task is `stopped` now, which an approval rejection may not act on.
    let before = events.len();
    assert!(
        persistence
            .stop_task(task.id, StopActor::Approval(approval_id))
            .await
            .is_err(),
        "a rejection must not stop a task it did not park"
    );
    let after = persistence
        .list_task_status_events(company.id, task.correlation_id, None, 50)
        .await
        .unwrap();
    assert_eq!(
        after.len(),
        before,
        "a matched-nothing stop writes no event"
    );

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// The database refuses attribution whose actor kind and ids disagree, so a call site cannot
/// record an approval-driven change as an operator's, or name two sources at once.
#[tokio::test]
async fn the_task_row_rejects_attribution_whose_shape_contradicts_its_actor_kind() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let task = enqueue_chain(&persistence, company.id, channel.id, "shape-check").await;

    // (what is wrong with it, reason, actor kind, actor id, approval id, outreach id)
    let refused = [
        (
            "an approval without the approval it names",
            "'approval_rejected'",
            "'approval'",
            "NULL",
            "NULL",
            "NULL",
        ),
        (
            "an outreach without the outreach it names",
            "'outreach_started'",
            "'outreach'",
            "NULL",
            "NULL",
            "NULL",
        ),
        (
            "an operator with no id",
            "'operator_stopped'",
            "'operator'",
            "NULL",
            "NULL",
            "NULL",
        ),
        (
            "a worker with no id",
            "'retryable_failure'",
            "'worker'",
            "NULL",
            "NULL",
            "NULL",
        ),
        (
            "the system claiming an actor",
            "'operator_stopped'",
            "'system'",
            "gen_random_uuid()",
            "NULL",
            "NULL",
        ),
        (
            "an operator carrying an approval",
            "'operator_stopped'",
            "'operator'",
            "gen_random_uuid()",
            "gen_random_uuid()",
            "NULL",
        ),
        (
            "two sources at once",
            "'approval_rejected'",
            "'approval'",
            "NULL",
            "gen_random_uuid()",
            "gen_random_uuid()",
        ),
        (
            "a reason with no actor kind at all",
            "'operator_stopped'",
            "NULL",
            "NULL",
            "NULL",
            "NULL",
        ),
        (
            "an actor with no reason",
            "NULL",
            "'operator'",
            "gen_random_uuid()",
            "NULL",
            "NULL",
        ),
    ];
    for (case, reason, kind, actor_id, approval_id, outreach_id) in refused {
        let outcome = sqlx::query(&format!(
            "UPDATE background_tasks
                    SET status = 'stopped',
                        transition_reason = {reason},
                        transition_actor_kind = {kind},
                        transition_actor_id = {actor_id},
                        transition_approval_id = {approval_id},
                        transition_outreach_id = {outreach_id}
                  WHERE id = $1"
        ))
        .bind(task.id)
        .execute(&pool)
        .await;
        assert!(outcome.is_err(), "{case} must be refused");
    }

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn pending_claims_take_one_company_round_before_a_second_task_from_a_backlog() {
    let _claim_guard = UNSCOPED_CLAIM.lock().await;
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let suffix = Uuid::new_v4().simple().to_string();
    let email = format!("fair_owner_{suffix}@example.com");
    persistence
        .create_user(&format!("fair_owner_{suffix}"), &email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(&persistence, &email)
        .await
        .unwrap()
        .unwrap();

    let mut companies = Vec::new();
    let mut channels = Vec::new();
    for label in ["backlog", "waiting"] {
        let company = CompanyPersistence::create(
            &persistence,
            owner.id,
            CompanyWrite {
                name: format!("Fair {label}"),
                slug: format!("fair-{label}-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();
        let agent_id = seed_channel_agent(&persistence, company.id, label).await;
        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: label.into(),
                slug: label.into(),
                agent_ids: Some(vec![agent_id]),
                enabled: false,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        companies.push(company);
        channels.push(channel);
    }

    let mut backlog_ids = Vec::new();
    for _ in 0..3 {
        backlog_ids.push(
            persistence
                .enqueue_task(NewTask::starting_new_chain(
                    companies[0].id,
                    channels[0].id,
                    None,
                    "fairness",
                    serde_json::json!({}),
                ))
                .await
                .unwrap()
                .id,
        );
    }
    let waiting = persistence
        .enqueue_task(NewTask::starting_new_chain(
            companies[1].id,
            channels[1].id,
            None,
            "fairness",
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    let mut owned_ids = backlog_ids.clone();
    owned_ids.push(waiting.id);
    sqlx::query(
            "UPDATE background_tasks SET run_at = CURRENT_TIMESTAMP - INTERVAL '300 years' WHERE id = ANY($1)",
        )
        .bind(&owned_ids)
        .execute(&pool)
        .await
        .unwrap();

    let claimed = persistence
        .claim_pending_tasks(Uuid::new_v4(), Utc::now() + chrono::Duration::minutes(5), 2)
        .await
        .unwrap();
    assert_eq!(
        claimed
            .iter()
            .filter(|task| task.company_id == companies[0].id)
            .count(),
        1
    );
    assert_eq!(
        claimed
            .iter()
            .filter(|task| task.company_id == companies[1].id)
            .count(),
        1
    );

    for company in companies {
        CompanyPersistence::delete(&persistence, company.id)
            .await
            .unwrap();
    }
}

/// The mailbox asks for a whole page of threads at once, and each thread must report the state
/// of its *current* run rather than whichever task happens to be found first.
#[tokio::test]
async fn thread_activity_reports_the_latest_task_per_thread() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let suffix = Uuid::new_v4().simple().to_string();
    let username = format!("activity_owner_{suffix}");
    let email = format!("{username}@example.com");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(&persistence, &email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        &persistence,
        owner.id,
        CompanyWrite {
            name: "Activity Test".to_string(),
            slug: format!("activity-test-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let channel = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "Activity".into(),
            slug: "activity".into(),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let email_addr = crate::entities::value_objects::EmailAddress::from(email.clone());

    let mut threads = Vec::new();
    for subject in ["running", "blocked", "finished", "superseded"] {
        threads.push(
            persistence
                .create_thread(channel.id, subject, std::slice::from_ref(&email_addr))
                .await
                .unwrap(),
        );
    }

    let enqueue = async |thread_id: Uuid| {
        persistence
            .enqueue_task(NewTask::starting_new_chain(
                company.id,
                channel.id,
                Some(thread_id),
                "email_agent_dispatch",
                serde_json::json!({}),
            ))
            .await
            .unwrap()
    };

    let running = enqueue(threads[0].id).await;
    let blocked = enqueue(threads[1].id).await;
    let finished = enqueue(threads[2].id).await;
    let old = enqueue(threads[3].id).await;
    let current = enqueue(threads[3].id).await;

    // `background_tasks_lease_check` gives the lease columns to `processing` rows and to no
    // other status, so they move together here exactly as the worker moves them.
    let set_status = async |id: Uuid, status: &str, lease: Option<DateTime<Utc>>| {
        sqlx::query(&format!(
                "UPDATE background_tasks
                 SET {CLEAR_TRANSITION},
                     status = $2,
                     lock_expires_at = $3,
                     worker_id = CASE WHEN $3::timestamptz IS NULL THEN NULL ELSE gen_random_uuid() END,
                     execution_generation =
                         CASE WHEN $3::timestamptz IS NULL THEN NULL ELSE gen_random_uuid() END,
                     -- Derived from the lease, not from now: the check also demands
                     -- lock_expires_at > locked_at, and an expired lease is set in the past.
                     locked_at = $3::timestamptz - interval '10 minutes',
                     updated_at = CURRENT_TIMESTAMP
                 WHERE id = $1"
            ))
            .bind(id)
            .bind(status)
            .bind(lease)
            .execute(&pool)
            .await
            .unwrap();
    };

    let live_lease = Utc::now() + chrono::Duration::minutes(5);
    set_status(running.id, "processing", Some(live_lease)).await;
    set_status(blocked.id, "pending_approval", None).await;
    set_status(finished.id, "completed", None).await;
    // Older task on the same thread ended badly; the newer one is what the reader should see.
    set_status(old.id, "dead_letter", None).await;
    set_status(current.id, "processing", Some(live_lease)).await;

    let ids: Vec<Uuid> = threads.iter().map(|thread| thread.id).collect();
    let activity = persistence.list_thread_work_summary(&ids).await.unwrap();

    assert_eq!(
        activity
            .get(&threads[0].id)
            .and_then(|summary| summary.activity.as_ref()),
        Some(&ThreadActivity::Working)
    );
    assert_eq!(
        activity
            .get(&threads[1].id)
            .and_then(|summary| summary.activity.as_ref()),
        Some(&ThreadActivity::WaitingApproval)
    );
    assert_eq!(
        activity.get(&threads[2].id),
        None,
        "a finished thread reports nothing at all, rather than an idle badge"
    );
    assert_eq!(
        activity
            .get(&threads[3].id)
            .and_then(|summary| summary.activity.as_ref()),
        Some(&ThreadActivity::Working),
        "the newest task wins over an older dead letter on the same thread"
    );

    // Asking again after a failure and getting an answer settles the thread: the run that
    // worked is its last word, and the dead letter behind it is history rather than a badge.
    set_status(current.id, "completed", None).await;
    let activity = persistence.list_thread_work_summary(&ids).await.unwrap();
    assert_eq!(
        activity.get(&threads[3].id),
        None,
        "a successful run buries the dead letter it was asked to make up for"
    );

    // The failure still stands on its own while nothing has answered it.
    set_status(current.id, "stopped", None).await;
    let activity = persistence.list_thread_work_summary(&ids).await.unwrap();
    assert_eq!(
        activity
            .get(&threads[3].id)
            .and_then(|summary| summary.activity.as_ref()),
        Some(&ThreadActivity::Failed),
        "a run that was stopped rather than answered leaves the failure showing"
    );
    set_status(current.id, "completed", None).await;

    // An abandoned worker leaves `processing` behind; that is queued work, not a live agent.
    set_status(
        running.id,
        "processing",
        Some(Utc::now() - chrono::Duration::minutes(5)),
    )
    .await;
    let activity = persistence.list_thread_work_summary(&ids).await.unwrap();
    assert_eq!(
        activity
            .get(&threads[0].id)
            .and_then(|summary| summary.activity.as_ref()),
        Some(&ThreadActivity::Queued)
    );

    assert!(
        persistence
            .list_thread_work_summary(&[])
            .await
            .unwrap()
            .is_empty(),
        "an empty page must not hit the database"
    );

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// An expired lease must cost an attempt, close its ledger row, and back the task off --
/// and eventually dead-letter it.
///
/// Regression for the shape where `claim_pending_tasks` stole expired `processing` rows
/// directly. That re-ran the task with `retry_count` untouched, left the abandoned attempt
/// sitting in `task_attempts` as `processing` for ever, and applied no backoff, so a task
/// that reliably outlived its lease was retried in a tight loop and never dead-lettered.
#[tokio::test]
async fn an_expired_task_lease_costs_an_attempt_and_eventually_dead_letters() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let suffix = Uuid::new_v4().simple().to_string();
    let username = format!("reaper_owner_{suffix}");
    let email = format!("{username}@example.com");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(&persistence, &email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        &persistence,
        owner.id,
        CompanyWrite {
            name: "Reaper Test".to_string(),
            slug: format!("reaper-test-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let agent_id = seed_channel_agent(&persistence, company.id, "reaper").await;
    let channel = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "Reaper".into(),
            slug: "reaper".into(),
            agent_ids: Some(vec![agent_id]),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let email_addr = crate::entities::value_objects::EmailAddress::from(email.clone());
    let thread = persistence
        .create_thread(channel.id, "Reaper", std::slice::from_ref(&email_addr))
        .await
        .unwrap();
    let task = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            channel.id,
            Some(thread.id),
            "test",
            serde_json::json!({}),
        ))
        .await
        .unwrap();

    // Run the task up to its retry ceiling, losing the lease every time.
    let max_retries = task.max_retries;
    let mut generations = Vec::new();
    for attempt in 1..=max_retries {
        // Due now, whatever backoff the previous reap applied.
        sqlx::query("UPDATE background_tasks SET run_at = CURRENT_TIMESTAMP WHERE id = $1")
            .bind(task.id)
            .execute(&pool)
            .await
            .unwrap();

        // By id rather than a batch claim: the batch sweeps the whole queue, so tests
        // running beside this one would fill it or take this row first.
        assert!(
            persistence
                .claim_task(
                    task.id,
                    Uuid::new_v4(),
                    Utc::now() + chrono::Duration::minutes(5)
                )
                .await
                .unwrap(),
            "the task is pending and due, so it must be claimable"
        );
        let claimed = persistence
            .get_task_by_id(task.id)
            .await
            .unwrap()
            .expect("the task still exists");
        let lease = TaskLeaseRef::of(&claimed).expect("a claim records its lease");
        generations.push(lease.execution_generation);
        persistence
            .begin_task_attempt(TaskAttemptRef::of(&claimed, lease), &test_machine())
            .await
            .unwrap();

        // The run vanishes: its lease lapses with nothing reported.
        sqlx::query(
            "UPDATE background_tasks
                 SET locked_at = CURRENT_TIMESTAMP - interval '20 minutes',
                     lock_expires_at = CURRENT_TIMESTAMP - interval '1 second'
                 WHERE id = $1",
        )
        .bind(task.id)
        .execute(&pool)
        .await
        .unwrap();

        // The sweep is global, so a test running beside this one may reap this row first.
        // What matters is the state the row ends in, not whose call got there.
        persistence.reap_expired_task_leases().await.unwrap();

        let after = persistence
            .get_task_by_id(task.id)
            .await
            .unwrap()
            .expect("the task still exists");
        assert_eq!(
            after.retry_count, attempt,
            "each lapsed lease must spend exactly one attempt"
        );
        assert!(after.worker_id.is_none());
        assert!(after.execution_generation.is_none());

        if attempt < max_retries {
            assert_eq!(after.status, TaskStatus::Pending);
            assert!(
                after.run_at > Utc::now(),
                "a reaped task must wait out its backoff"
            );
        } else {
            assert_eq!(
                after.status,
                TaskStatus::DeadLetter,
                "the attempt budget is spent, so the task must stop rather than loop"
            );
        }
    }

    // Every claim minted its own generation, which is what fences a superseded run.
    let unique = generations.iter().collect::<std::collections::HashSet<_>>();
    assert_eq!(
        unique.len(),
        generations.len(),
        "each claim must mint a distinct execution generation"
    );

    // No attempt was left open: the reaper closed each one as it went.
    let open: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM task_attempts WHERE task_id = $1 AND status = 'processing'",
    )
    .bind(task.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(open, 0, "a reaped run must not leave its ledger row open");
    let lease_lost: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM task_attempts WHERE task_id = $1 AND stop_reason = $2",
    )
    .bind(task.id)
    .bind(TaskStopReason::LeaseLost.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        lease_lost,
        i64::from(task.max_retries),
        "every reaped attempt records why execution stopped"
    );

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// A run whose lease was reaped must not be able to write anything, even if the same worker
/// id re-claims the task. Only the generation can tell those two runs apart.
#[tokio::test]
async fn a_superseded_run_cannot_renew_write_or_close_the_task() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let suffix = Uuid::new_v4().simple().to_string();
    let username = format!("fence_owner_{suffix}");
    let email = format!("{username}@example.com");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(&persistence, &email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        &persistence,
        owner.id,
        CompanyWrite {
            name: "Fence Test".to_string(),
            slug: format!("fence-test-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let agent_id = seed_channel_agent(&persistence, company.id, "fence").await;
    let channel = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "Fence".into(),
            slug: "fence".into(),
            agent_ids: Some(vec![agent_id]),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let email_addr = crate::entities::value_objects::EmailAddress::from(email.clone());
    let thread = persistence
        .create_thread(channel.id, "Fence", std::slice::from_ref(&email_addr))
        .await
        .unwrap();
    let task = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            channel.id,
            Some(thread.id),
            "test",
            serde_json::json!({}),
        ))
        .await
        .unwrap();

    // Deliberately the *same* worker both times, so only the generation differs. This is the
    // case a `worker_id = $me` guard cannot catch.
    let worker = Uuid::new_v4();

    // By id rather than a batch claim: the batch sweeps the whole queue, so a test running
    // beside this one could take this row first.
    assert!(
        persistence
            .claim_task(task.id, worker, Utc::now() + chrono::Duration::minutes(5))
            .await
            .unwrap()
    );
    let first = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
    let stale = TaskLeaseRef::of(&first).expect("a claim records its lease");

    sqlx::query(
        "UPDATE background_tasks
             SET locked_at = CURRENT_TIMESTAMP - interval '20 minutes',
                 lock_expires_at = CURRENT_TIMESTAMP - interval '1 second'
             WHERE id = $1",
    )
    .bind(task.id)
    .execute(&pool)
    .await
    .unwrap();
    persistence.reap_expired_task_leases().await.unwrap();

    sqlx::query("UPDATE background_tasks SET run_at = CURRENT_TIMESTAMP WHERE id = $1")
        .bind(task.id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        persistence
            .claim_task(task.id, worker, Utc::now() + chrono::Duration::minutes(5))
            .await
            .unwrap()
    );
    let second = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
    let current = TaskLeaseRef::of(&second).expect("a claim records its lease");

    assert_eq!(stale.worker_id, current.worker_id, "same worker both times");
    assert_ne!(stale.execution_generation, current.execution_generation);

    // Nothing the superseded run tries may land.
    assert!(
        !persistence
            .renew_task_lease(stale, Utc::now() + chrono::Duration::minutes(5))
            .await
            .unwrap()
    );
    assert!(!persistence.mark_task_completed(stale).await.unwrap());
    assert!(
        !persistence
            .mark_task_failed(TaskFailure {
                lease: stale,
                error: "stale",
                next_run_at: Utc::now(),
                outcome: TaskFailureOutcome::Retry,
                reason: TaskStopReason::RetryableFailure,
            })
            .await
            .unwrap()
    );

    // The payload the superseded run tried to write never landed.
    let after = persistence
        .get_task_by_id(task.id)
        .await
        .unwrap()
        .expect("the task still exists");
    assert_eq!(after.status, TaskStatus::Processing);
    assert!(after.payload.get("stale").is_none());

    // The run that actually owns the task is unaffected.
    assert!(persistence.mark_task_completed(current).await.unwrap());

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// One dispatch's reply, the delivery that carries it and its task payload land together or not
/// at all.
///
/// They used to be three independent commits: the queue row, then a `create_message` per
/// answered thread, then the payload. A crash or a lost lease part-way left a thread showing
/// an answer that was never sent, or an email going out for a task whose payload said it had
/// never run -- and the retry then had to reconcile the difference.
#[tokio::test]
async fn a_dispatch_commits_its_reply_delivery_and_payload_together_or_not_at_all() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let suffix = Uuid::new_v4().simple().to_string();
    let username = format!("commit_owner_{suffix}");
    let email = format!("{username}@example.com");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(&persistence, &email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        &persistence,
        owner.id,
        CompanyWrite {
            name: "Commit Test".to_string(),
            slug: format!("commit-test-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let agent_id = seed_channel_agent(&persistence, company.id, "commit").await;
    let channel = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "Commit".into(),
            slug: "commit".into(),
            agent_ids: Some(vec![agent_id]),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let email_addr = crate::entities::value_objects::EmailAddress::from(email.clone());
    let thread = persistence
        .create_thread(channel.id, "Commit", std::slice::from_ref(&email_addr))
        .await
        .unwrap();
    let task = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            channel.id,
            Some(thread.id),
            "test",
            serde_json::json!({}),
        ))
        .await
        .unwrap();

    let worker = Uuid::new_v4();
    assert!(
        persistence
            .claim_task(task.id, worker, Utc::now() + chrono::Duration::minutes(5))
            .await
            .unwrap()
    );
    let claimed = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
    let lease = TaskLeaseRef::of(&claimed).expect("a claim records its lease");

    // The agent's answer as the dispatch now writes it: one canonical message, authored by the
    // platform, with no mail headers on it at all.
    let reply = |thread_id: Uuid| AgentReply {
        message: MessageWrite::internal(
            thread_id,
            MessageAuthorWrite::Platform,
            "Re: Commit",
            "the answer",
            MessageDirection::Outbound,
            MessageRole::Agent,
            CorrelationId::new(),
        )
        .external_conversation(),
        also_in_threads: Vec::new(),
    };
    // One delivery per logical send. Two distinct keys, and then the *same* key twice, which is
    // what proves the unique index absorbs a re-run rather than queueing the answer again.
    // The fixture writes a message of its own, so it goes in a side thread: the assertions below
    // count outbound messages in `thread`, and a fixture that wrote there would be counting itself.
    let side_thread = persistence
        .create_thread(channel.id, "Fixtures", &[])
        .await
        .unwrap();
    let delivery = async |key: &str, message_id| -> NewDelivery {
        let mut queued = delivery_fixture(
            &persistence,
            DeliveryFixtureRequest {
                task_id: Some(task.id),
                ..DeliveryFixtureRequest::new(company.id, channel.id, side_thread.id, key)
            },
        )
        .await
        .delivery;
        // The delivery carries the reply the commit is about to store, not the fixture's own
        // message: `create_message_with_deliveries` and the dispatch commit both refuse a
        // delivery that names anything else.
        queued.message_id = message_id;
        queued
    };

    let delivery_rows = async || -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM message_deliveries WHERE task_id = $1")
            .bind(task.id)
            .fetch_one(&pool)
            .await
            .unwrap()
    };
    let thread_rows = async || -> i64 {
        sqlx::query_scalar(
            r#"SELECT COUNT(*)
               FROM thread_messages AS association
               JOIN messages AS message
                 ON (message.company_id, message.id) =
                    (association.company_id, association.message_id)
               WHERE association.thread_id = $1 AND message.direction = 'outbound'"#,
        )
        .bind(thread.id)
        .fetch_one(&pool)
        .await
        .unwrap()
    };

    // A superseded run: the same task and worker, a generation that is no longer current.
    let stale = TaskLeaseRef {
        execution_generation: Uuid::new_v4(),
        ..lease
    };
    let stale_reply = reply(thread.id);
    let outcome = persistence
        .commit_agent_dispatch(AgentDispatchCommit {
            lease: stale,
            reply: &stale_reply,
            deliveries: vec![delivery("stale-key", stale_reply.message.id).await],
            review_candidate: None,
            payload: serde_json::json!({"stale": true}),
            complete_outreach: false,
        })
        .await
        .unwrap();
    assert_eq!(outcome, DispatchCommit::LeaseLost);

    // Not one of the three parts may have landed.
    assert_eq!(thread_rows().await, 0, "no reply may be stored");
    assert_eq!(delivery_rows().await, 0, "nothing may be queued");
    let after_stale = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
    assert!(
        after_stale.payload.get("stale").is_none(),
        "no payload may be written"
    );

    // The run that actually owns the lease commits all three.
    let live_reply = reply(thread.id);
    let live_delivery = delivery("live-key", live_reply.message.id).await;
    let outcome = persistence
        .commit_agent_dispatch(AgentDispatchCommit {
            lease,
            reply: &live_reply,
            deliveries: vec![live_delivery.clone()],
            review_candidate: None,
            payload: serde_json::json!({"committed": true}),
            complete_outreach: false,
        })
        .await
        .unwrap();
    assert_eq!(
        outcome,
        DispatchCommit::Committed {
            deliveries: vec![DeliveryCreation::Created(live_delivery.id)]
        }
    );
    assert_eq!(thread_rows().await, 1);
    assert_eq!(delivery_rows().await, 1);
    let after = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
    assert_eq!(
        after.payload.get("committed"),
        Some(&serde_json::json!(true))
    );

    // Re-queueing the same logical send is the idempotency key doing its job, not a failure, and
    // it must not duplicate the delivery. A superseded run mints a *new* message id, which is
    // exactly why the key is derived from the task rather than from the message.
    let rerun_reply = reply(thread.id);
    let rerun_delivery = delivery("live-key", rerun_reply.message.id).await;
    let outcome = persistence
        .commit_agent_dispatch(AgentDispatchCommit {
            lease,
            reply: &rerun_reply,
            deliveries: vec![rerun_delivery],
            review_candidate: None,
            payload: serde_json::json!({"committed": true}),
            complete_outreach: false,
        })
        .await
        .unwrap();
    assert_eq!(
        outcome,
        DispatchCommit::Committed {
            deliveries: vec![DeliveryCreation::Absorbed(live_delivery.id)]
        },
        "the second run must attach to the delivery the first one queued"
    );
    assert_eq!(
        delivery_rows().await,
        1,
        "the same send must not queue twice"
    );

    // A failure part-way must roll back what already succeeded in the same transaction. The
    // payload write happens first, so a message that cannot be stored has to undo it: this is
    // the case three separate commits could not handle at all.
    let orphan = reply(Uuid::new_v4());
    let orphan_delivery = delivery("orphan-key", orphan.message.id).await;
    let failed = persistence
        .commit_agent_dispatch(AgentDispatchCommit {
            lease,
            reply: &orphan,
            deliveries: vec![orphan_delivery],
            review_candidate: None,
            payload: serde_json::json!({"rolled_back": true}),
            complete_outreach: false,
        })
        .await;
    assert!(failed.is_err(), "a message with no thread cannot be stored");

    let after_rollback = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
    assert!(
        after_rollback.payload.get("rolled_back").is_none(),
        "the payload write must be rolled back when a later write fails"
    );
    assert_eq!(
        after_rollback.payload.get("committed"),
        Some(&serde_json::json!(true)),
        "and the previously committed payload must survive untouched"
    );
    assert_eq!(
        delivery_rows().await,
        1,
        "the failed dispatch must not leave a delivery behind"
    );

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn concurrent_workers_claim_once_and_a_failed_task_is_not_immediately_reclaimed() {
    // Both claims below are unscoped, and the row is deliberately sorted to the very front of
    // the queue -- which makes it the first thing any *other* unscoped claim takes too. Held
    // from before the row is queued until after the last claim, so the only claims racing for
    // it are this test's own two.
    let _claim_guard = UNSCOPED_CLAIM.lock().await;
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let suffix = Uuid::new_v4().simple().to_string();
    let username = format!("queue_owner_{suffix}");
    let email = format!("{username}@example.com");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(&persistence, &email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        &persistence,
        owner.id,
        CompanyWrite {
            name: "Queue Test".to_string(),
            slug: format!("queue-test-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let agent_id = seed_channel_agent(&persistence, company.id, "queue").await;
    let channel = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "Queue".into(),
            slug: "queue".into(),
            agent_ids: Some(vec![agent_id]),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let email_addr = crate::entities::value_objects::EmailAddress::from(email.clone());
    let thread = persistence
        .create_thread(channel.id, "Queue", std::slice::from_ref(&email_addr))
        .await
        .unwrap();
    let task = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            channel.id,
            Some(thread.id),
            "test",
            serde_json::json!({}),
        ))
        .await
        .unwrap();

    // `claim_pending_tasks` polls the whole queue, not this company's slice of it. Its first
    // sort key is the per-company round, and a brand-new company's only task is always round 1
    // — so sorting this row ahead of every other round-1 row is what puts it first overall,
    // ahead of concurrent tests' rows and any orphans a previously aborted run left behind.
    // Without it both single-slot workers fill up elsewhere and never reach this task. Keeping
    // the limit at 1 also means this test steals at most one foreign task.
    sqlx::query(
            "UPDATE background_tasks SET run_at = CURRENT_TIMESTAMP - INTERVAL '100 years' WHERE id = $1",
        )
        .bind(task.id)
        .execute(&pool)
        .await
        .unwrap();

    let first_worker = Uuid::new_v4();
    let second_worker = Uuid::new_v4();
    let expires_at = chrono::Utc::now() + chrono::Duration::minutes(5);
    let (first, second) = tokio::join!(
        persistence.claim_pending_tasks(first_worker, expires_at, 1),
        persistence.claim_pending_tasks(second_worker, expires_at, 1)
    );
    let claimed: Vec<_> = first.unwrap().into_iter().chain(second.unwrap()).collect();

    // The invariant that matters is that *this* task went to exactly one worker — asserting on
    // the combined queue total would count whatever else the other worker legitimately claimed.
    assert_eq!(
        claimed.iter().filter(|claim| claim.id == task.id).count(),
        1,
        "a pending task must be claimed by exactly one worker"
    );
    let events = persistence
        .list_task_status_events(company.id, task.correlation_id, None, 20)
        .await
        .unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.reason == TaskTransitionReason::Claimed)
            .count(),
        1,
        "competing claimants must produce one claimed transition event"
    );

    // Two claims, one of which is ours by construction — so the other necessarily took someone
    // else's task, and it now holds a five-minute lease on it. Hand it straight back: whichever
    // test queued it is about to find its own task already claimed and fail on a state it never
    // set. Releasing to 'pending' is where a reaped lease lands anyway.
    let borrowed: Vec<Uuid> = claimed
        .iter()
        .map(|claim| claim.id)
        .filter(|id| *id != task.id)
        .collect();
    if !borrowed.is_empty() {
        sqlx::query(&format!(
            "UPDATE background_tasks
                    SET {CLEAR_TRANSITION}, status = 'pending', worker_id = NULL,
                        execution_generation = NULL, locked_at = NULL, lock_expires_at = NULL
                  WHERE id = ANY($1)"
        ))
        .bind(&borrowed)
        .execute(&pool)
        .await
        .unwrap();
    }

    // This single row fills the worker's one-task batch. Failing it must move it behind
    // persisted backoff before `MoreWaiting` sends the worker straight into another
    // iteration; otherwise a poison task is reclaimed without any clock advance.
    let claimed_task = claimed
        .iter()
        .find(|claim| claim.id == task.id)
        .expect("this task was claimed");
    let claimed_lease = TaskLeaseRef::of(claimed_task).expect("a claimed task records its lease");
    assert!(
        persistence
            .mark_task_failed(TaskFailure {
                lease: claimed_lease,
                error: "poison task",
                next_run_at: Utc::now() + chrono::Duration::minutes(1),
                outcome: TaskFailureOutcome::Retry,
                reason: TaskStopReason::RetryableFailure,
            })
            .await
            .unwrap()
    );

    let immediate_worker = Uuid::new_v4();
    let immediate = persistence
        .claim_pending_tasks(
            immediate_worker,
            Utc::now() + chrono::Duration::minutes(5),
            1,
        )
        .await
        .unwrap();
    assert!(
        immediate.iter().all(|claim| claim.id != task.id),
        "a failed full batch must not reclaim the same task on the zero-delay iteration"
    );
    let immediate_borrowed: Vec<Uuid> = immediate.iter().map(|claim| claim.id).collect();
    if !immediate_borrowed.is_empty() {
        sqlx::query(&format!(
            "UPDATE background_tasks
                    SET {CLEAR_TRANSITION}, status = 'pending', worker_id = NULL,
                        execution_generation = NULL, locked_at = NULL, lock_expires_at = NULL
                  WHERE id = ANY($1) AND worker_id = $2"
        ))
        .bind(&immediate_borrowed)
        .bind(immediate_worker)
        .execute(&pool)
        .await
        .unwrap();
    }

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn outreach_reply_reaches_quorum_and_resumes_task() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let suffix = Uuid::new_v4().simple().to_string();
    let owner_email = format!("outreach_owner_{suffix}@example.com");
    persistence
        .create_user(&format!("outreach_owner_{suffix}"), &owner_email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(&persistence, &owner_email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        &persistence,
        owner.id,
        CompanyWrite {
            name: "Outreach Test".to_string(),
            slug: format!("outreach-test-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let agent_id = seed_channel_agent(&persistence, company.id, "outreach").await;
    let channel = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "Outreach".into(),
            slug: "outreach".into(),
            agent_ids: Some(vec![agent_id]),
            participant_emails: Some(vec![owner_email.clone()]),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let owner_email_addr = crate::entities::value_objects::EmailAddress::from(owner_email.clone());
    let thread = persistence
        .create_thread(
            channel.id,
            "Need response",
            std::slice::from_ref(&owner_email_addr),
        )
        .await
        .unwrap();
    let task = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            channel.id,
            Some(thread.id),
            "email_agent_dispatch",
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    let worker_id = Uuid::new_v4();
    assert!(
        persistence
            .claim_task(
                task.id,
                worker_id,
                chrono::Utc::now() + chrono::Duration::minutes(5),
            )
            .await
            .unwrap()
    );
    let lease =
        TaskLeaseRef::of(&persistence.get_task_by_id(task.id).await.unwrap().unwrap()).unwrap();
    let outreach_id = Uuid::new_v4();
    let target_email = "vendor@supplier.example";
    let asked = delivery_fixture(
        &persistence,
        DeliveryFixtureRequest {
            task_id: Some(task.id),
            recipient: target_email,
            subject: "Question",
            body: "Please respond",
            purpose: DeliveryPurpose::Outreach,
            ..DeliveryFixtureRequest::new(company.id, channel.id, thread.id, "outreach-target-0")
        },
    )
    .await;
    let delivery_id = asked.delivery.id;
    let progress = persistence
        .create_outreach_and_pause(CreateOutreachRequest {
            invocation: None,
            correlation_id: CorrelationId::new(),
            id: outreach_id,
            lease,
            company_id: company.id,
            channel_id: channel.id,
            outreach_key: "integration-outreach".into(),
            required_threshold_percent: 100.0,
            expires_at: chrono::Utc::now() + chrono::Duration::hours(24),
            subject: "Question".into(),
            body: "Please respond".into(),
            targets: vec![crate::task_queue::OutreachTargetRequest {
                target: crate::task_queue::OutreachTargetIdentity::External {
                    identity: qualified_email_identity(target_email).unwrap(),
                },
                request: email_write(EmailMessageDraft {
                    id: Uuid::new_v4(),
                    thread_id: thread.id,
                    message_id: "<asked-vendor@mailagents.test>".into(),
                    sender: "support@acme.mailagents.test".into(),
                    recipients_to: vec![target_email.into()],
                    subject: "Question".into(),
                    clean_text_body: "Please respond".into(),
                    direction: MessageDirection::Outbound,
                    role: MessageRole::Agent,
                    ..EmailMessageDraft::default()
                }),
                delivery: asked.delivery.clone(),
            }],
        })
        .await
        .unwrap();
    assert!(progress.suspended);
    let summary = persistence
        .get_collaboration_summary(
            CollaborationReadScope {
                company_id: company.id,
                visible_channel_ids: &[channel.id],
            },
            task.id,
        )
        .await
        .unwrap()
        .expect("the viewer can read the owning task");
    assert_eq!(
        summary.status,
        crate::entities::collaboration::OutreachBusinessStatus::Waiting
    );
    assert_eq!(summary.progress.unwrap().total, 1);
    assert_eq!(summary.children.len(), 1);
    assert_eq!(
        summary.children[0].target,
        crate::entities::collaboration::CollaborationTarget::External {
            identity: qualified_email_identity(target_email).unwrap(),
        }
    );
    assert_eq!(
        summary.children[0].status,
        crate::entities::collaboration::TargetBusinessStatus::Sending
    );
    assert!(
        persistence
            .get_collaboration_summary(
                CollaborationReadScope {
                    company_id: Uuid::new_v4(),
                    visible_channel_ids: &[channel.id],
                },
                task.id,
            )
            .await
            .unwrap()
            .is_none(),
        "a task id cannot escape its company scope"
    );
    assert_eq!(
        persistence
            .get_task_by_id(task.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::WaitingForThirdPartyReply
    );
    let started = persistence
        .list_task_status_events(company.id, task.correlation_id, None, 20)
        .await
        .unwrap()
        .into_iter()
        .find(|event| event.reason == TaskTransitionReason::OutreachStarted)
        .expect("outreach suspension records its source");
    assert_eq!(started.related_outreach_id, Some(outreach_id));

    // The reply guard matches on the provider key the question actually went out under, so the
    // part has to be delivered before a reply can be correlated to it.
    let outbound_message_id = "<outreach-vendor@mailagents.test>";
    sqlx::query(
        "UPDATE message_delivery_parts
            SET status = 'delivered', provider_message_key = $2,
                request_started_at = CURRENT_TIMESTAMP, delivered_at = CURRENT_TIMESTAMP
          WHERE delivery_id = $1",
    )
    .bind(delivery_id.as_uuid())
    .bind(outbound_message_id)
    .execute(&persistence.pool)
    .await
    .unwrap();
    let matched = persistence
        .find_correlated_outreach_reply(
            company.id,
            channel.id,
            thread.id,
            target_email,
            &[outbound_message_id.into()],
        )
        .await
        .unwrap()
        .unwrap();
    let response = persistence
        .create_message(&email_write(EmailMessageDraft {
            id: Uuid::new_v4(),
            thread_id: thread.id,
            message_id: "<vendor-response@supplier.example>".into(),
            in_reply_to: Some(outbound_message_id.into()),
            references_list: vec![outbound_message_id.into()],
            sender: target_email.into(),
            recipients_to: vec![owner_email.clone().into()],
            recipients_cc: Vec::new(),
            subject: "Re: Question".into(),
            clean_text_body: "Confirmed".into(),
            raw_text_body: None,
            raw_html_body: None,
            attachments: None,
            direction: MessageDirection::Inbound,
            role: MessageRole::Human,
            thread_index: None,
            created_at: chrono::Utc::now(),
        }))
        .await
        .unwrap();
    let progress = persistence
        .record_outreach_reply(&matched, response.id)
        .await
        .unwrap();
    assert_eq!(progress.status, OutreachStatus::ThresholdMet);
    assert_eq!(progress.response_count, 1);
    let summary = persistence
        .get_collaboration_summary(
            CollaborationReadScope {
                company_id: company.id,
                visible_channel_ids: &[channel.id],
            },
            task.id,
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        summary.status,
        crate::entities::collaboration::OutreachBusinessStatus::ReadyToResume
    );
    assert_eq!(
        summary.children[0].status,
        crate::entities::collaboration::TargetBusinessStatus::Responded
    );
    assert_eq!(
        persistence
            .get_task_by_id(task.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Pending
    );
    let replied = persistence
        .list_task_status_events(company.id, task.correlation_id, None, 20)
        .await
        .unwrap()
        .into_iter()
        .find(|event| event.reason == TaskTransitionReason::OutreachReplyReceived)
        .expect("outreach reply records the exact resumption reason");
    assert_eq!(replied.related_outreach_id, Some(outreach_id));

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn collaboration_targets_reject_cross_company_internal_channels() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let (first_company, first_channel) = seed_company_and_channel(&persistence).await;
    let (second_company, second_channel) = seed_company_and_channel(&persistence).await;
    let task = enqueue_chain(
        &persistence,
        first_company.id,
        first_channel.id,
        "cross-company-collaboration-target",
    )
    .await;
    let outreach_id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO task_outreaches
               (id, task_id, company_id, status, required_threshold_percent, expires_at,
                outreach_key, subject, body)
           VALUES ($1, $2, $3, 'waiting', 100, CURRENT_TIMESTAMP + interval '1 day',
                   'cross-company-target', 'Question', 'Body')"#,
    )
    .bind(outreach_id)
    .bind(task.id)
    .bind(first_company.id)
    .execute(&pool)
    .await
    .unwrap();

    let error = sqlx::query(
        r#"INSERT INTO task_outreach_targets
               (outreach_id, company_id, email, target_kind, internal_channel_id)
           VALUES ($1, $2, 'other@example.test', 'internal_channel', $3)"#,
    )
    .bind(outreach_id)
    .bind(first_company.id)
    .bind(second_channel.id)
    .execute(&pool)
    .await
    .expect_err("another company's valid channel id must be rejected");
    assert_eq!(
        error.as_database_error().and_then(|error| error.code()),
        Some(std::borrow::Cow::Borrowed("23503"))
    );

    CompanyPersistence::delete(&persistence, first_company.id)
        .await
        .unwrap();
    CompanyPersistence::delete(&persistence, second_company.id)
        .await
        .unwrap();
}

/// The outreach mail the agent sent lands in the thread *and* is marked as the question, in one
/// transaction.
///
/// The mark is what stops the reply guard reading it as the answer this turn owed. Written
/// separately, a failure between the two would leave an unmarked outreach mail in the thread, and
/// the next attempt would find it, call the work done, and complete the task with no answer.
/// Reaching quorum retires the questions the outreach never sent.
///
/// The shape this replaces asked "is this delivery still wanted?" once per claimed row -- a round
/// trip per send, answering from state that could change a millisecond later. Deciding in the
/// transaction that closes the outreach is both cheaper and correct: a worker cannot claim a
/// question in between.
#[tokio::test]
async fn quorum_retires_the_outreach_questions_that_were_never_sent() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let thread = persistence
        .create_thread(channel.id, "Quorum", &[])
        .await
        .unwrap();
    let task = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            channel.id,
            Some(thread.id),
            "email_agent_dispatch",
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    let worker_id = Uuid::new_v4();
    assert!(
        persistence
            .claim_task(
                task.id,
                worker_id,
                Utc::now() + chrono::Duration::minutes(5)
            )
            .await
            .unwrap()
    );
    let lease =
        TaskLeaseRef::of(&persistence.get_task_by_id(task.id).await.unwrap().unwrap()).unwrap();

    // Two targets, a quorum of one: the second question is queued and then never wanted.
    let suffix = Uuid::new_v4().simple().to_string();
    let mut targets = Vec::new();
    let mut deliveries = Vec::new();
    for (index, address) in ["first@partner.test", "second@partner.test"]
        .into_iter()
        .enumerate()
    {
        let queued = delivery_fixture(
            &persistence,
            DeliveryFixtureRequest {
                task_id: Some(task.id),
                recipient: address,
                purpose: DeliveryPurpose::Outreach,
                ..DeliveryFixtureRequest::new(
                    company.id,
                    channel.id,
                    thread.id,
                    &format!("quorum-{suffix}-{index}"),
                )
            },
        )
        .await;
        deliveries.push(queued.delivery.id);
        targets.push(crate::task_queue::OutreachTargetRequest {
            target: crate::task_queue::OutreachTargetIdentity::External {
                identity: qualified_email_identity(address).unwrap(),
            },
            request: email_write(EmailMessageDraft {
                id: Uuid::new_v4(),
                thread_id: thread.id,
                message_id: format!("<quorum-{suffix}-{index}@mailagents.test>").into(),
                sender: "support@acme.mailagents.test".into(),
                recipients_to: vec![address.into()],
                subject: "Question".into(),
                clean_text_body: "Please respond".into(),
                direction: MessageDirection::Outbound,
                role: MessageRole::Agent,
                ..EmailMessageDraft::default()
            }),
            delivery: queued.delivery,
        });
    }

    let outreach_id = Uuid::new_v4();
    persistence
        .create_outreach_and_pause(CreateOutreachRequest {
            invocation: None,
            correlation_id: task.correlation_id,
            id: outreach_id,
            lease,
            company_id: company.id,
            channel_id: channel.id,
            outreach_key: format!("quorum-{suffix}"),
            required_threshold_percent: 50.0,
            expires_at: Utc::now() + chrono::Duration::hours(24),
            subject: "Question".into(),
            body: "Please respond".into(),
            targets,
        })
        .await
        .unwrap();

    // The first target's question goes out and is answered.
    mark_delivered(&pool, deliveries[0].as_uuid()).await;
    let response = persistence
        .create_message(&email_write(EmailMessageDraft {
            id: Uuid::new_v4(),
            thread_id: thread.id,
            message_id: format!("<answer-{suffix}@partner.test>").into(),
            sender: "first@partner.test".into(),
            subject: "Re: Question".into(),
            clean_text_body: "Confirmed".into(),
            direction: MessageDirection::Inbound,
            role: MessageRole::Human,
            ..EmailMessageDraft::default()
        }))
        .await
        .unwrap();
    let progress = persistence
        .record_outreach_reply(
            &crate::entities::outreach::OutreachReplyMatch {
                outreach_id,
                task_id: task.id,
                target_id: sqlx::query_scalar(
                    "SELECT id FROM task_outreach_targets WHERE outreach_id = $1 AND email = $2",
                )
                .bind(outreach_id)
                .bind("first@partner.test")
                .fetch_one(&pool)
                .await
                .unwrap(),
                target_email: "first@partner.test".into(),
            },
            response.id,
        )
        .await
        .unwrap();
    assert_eq!(progress.status, OutreachStatus::ThresholdMet);

    let second: String = sqlx::query_scalar("SELECT status FROM message_deliveries WHERE id = $1")
        .bind(deliveries[1].as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        second,
        DeliveryStatus::DeadLetter.as_str(),
        "a question nobody is waiting on any more must not go out"
    );
    let reason: Option<String> =
        sqlx::query_scalar("SELECT last_error_class FROM message_deliveries WHERE id = $1")
            .bind(deliveries[1].as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        reason.as_deref(),
        Some("superseded"),
        "and it must say why, rather than reading as a delivery that failed"
    );
    // The one that did go out keeps its result: cancelling is not a sweep over everything.
    let first: String = sqlx::query_scalar("SELECT status FROM message_deliveries WHERE id = $1")
        .bind(deliveries[0].as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(first, DeliveryStatus::Delivered.as_str());

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn an_outreach_request_message_and_its_mark_land_together() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let thread = persistence
        .create_thread(channel.id, "Need response", &[])
        .await
        .unwrap();
    let task = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            channel.id,
            Some(thread.id),
            "email_agent_dispatch",
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    let worker_id = Uuid::new_v4();
    assert!(
        persistence
            .claim_task(
                task.id,
                worker_id,
                chrono::Utc::now() + chrono::Duration::minutes(5),
            )
            .await
            .unwrap()
    );
    let lease =
        TaskLeaseRef::of(&persistence.get_task_by_id(task.id).await.unwrap().unwrap()).unwrap();

    let suffix = Uuid::new_v4().simple().to_string();
    let queued = delivery_fixture(
        &persistence,
        DeliveryFixtureRequest {
            task_id: Some(task.id),
            recipient: "vendor@supplier.example",
            purpose: DeliveryPurpose::Outreach,
            ..DeliveryFixtureRequest::new(
                company.id,
                channel.id,
                thread.id,
                &format!("mark-{suffix}"),
            )
        },
    )
    .await;
    let delivery_id = queued.delivery.id;

    let trigger = persistence
        .create_message(&email_write(EmailMessageDraft {
            id: Uuid::new_v4(),
            thread_id: thread.id,
            message_id: format!("<trigger-{suffix}@example.com>").into(),
            sender: "asker@example.com".into(),
            subject: "Question".into(),
            clean_text_body: "Please find out".into(),
            direction: MessageDirection::Inbound,
            role: MessageRole::Human,
            ..EmailMessageDraft::default()
        }))
        .await
        .unwrap();

    // The question, the mail that carries it, and the mark that says it *is* the question, all in
    // the one transaction that creates the outreach.
    let question = email_write(EmailMessageDraft {
        id: Uuid::new_v4(),
        thread_id: thread.id,
        message_id: format!("<outreach-{suffix}@mailagents.test>").into(),
        sender: "support@acme.mailagents.test".into(),
        recipients_to: vec!["vendor@supplier.example".into()],
        subject: "Question".into(),
        clean_text_body: "Please respond".into(),
        direction: MessageDirection::Outbound,
        role: MessageRole::Agent,
        created_at: chrono::Utc::now() + chrono::Duration::seconds(1),
        ..EmailMessageDraft::default()
    });
    let asked = question.id;
    persistence
        .create_outreach_and_pause(CreateOutreachRequest {
            invocation: None,
            correlation_id: CorrelationId::new(),
            id: Uuid::new_v4(),
            lease,
            company_id: company.id,
            channel_id: channel.id,
            outreach_key: format!("mark-{suffix}"),
            required_threshold_percent: 100.0,
            expires_at: chrono::Utc::now() + chrono::Duration::hours(24),
            subject: "Question".into(),
            body: "Please respond".into(),
            targets: vec![crate::task_queue::OutreachTargetRequest {
                target: crate::task_queue::OutreachTargetIdentity::External {
                    identity: qualified_email_identity("vendor@supplier.example").unwrap(),
                },
                request: question,
                delivery: queued.delivery.clone(),
            }],
        })
        .await
        .unwrap();

    // The mail is in the thread, so somebody reading the conversation sees what was asked.
    assert!(
        persistence
            .list_thread_message_views(thread.id)
            .await
            .unwrap()
            .iter()
            .any(|view| view.canonical_id == asked),
        "the outreach's own mail belongs in the thread"
    );
    // And it is marked as the question, so the guard does not read it as the answer.
    let marked: Option<Uuid> = sqlx::query_scalar(
        "SELECT request_message_id FROM task_outreach_targets WHERE delivery_id = $1",
    )
    .bind(delivery_id.as_uuid())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(marked, Some(asked.as_uuid()));
    assert!(
        persistence
            .find_outbound_reply_after(thread.id, trigger.canonical_id)
            .await
            .unwrap()
            .is_none(),
        "an outreach's own mail is the agent asking, not the agent answering"
    );

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

async fn seed_channel_agent(
    persistence: &PostgresPersistence,
    company_id: Uuid,
    label: &str,
) -> Uuid {
    let suffix = Uuid::new_v4().simple().to_string();
    AgentPersistence::create(
        persistence,
        company_id,
        AgentWrite {
            name: format!("{label} agent"),
            slug: format!("{label}-agent-{suffix}"),
            created_by: Some(CreationProvenance::system()),
            harness_kind: Some(crate::entities::harness::HarnessKind::AiAgents),
            ..Default::default()
        },
    )
    .await
    .expect("test channel agent is created")
    .id
}

/// A company and an enabled channel to hang tasks off, with a unique slug per call so
/// database-backed tests do not collide with each other or with a previous run.
async fn seed_company_and_channel(
    persistence: &PostgresPersistence,
) -> (
    crate::entities::company::Company,
    crate::entities::channel::Channel,
) {
    let suffix = Uuid::new_v4().simple().to_string();
    let username = format!("chain_owner_{suffix}");
    let email = format!("{username}@example.com");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(persistence, &email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        persistence,
        owner.id,
        CompanyWrite {
            name: "Chain Test".to_string(),
            slug: format!("chain-test-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let agent = AgentPersistence::create(
        persistence,
        company.id,
        AgentWrite {
            name: "Chain Agent".into(),
            slug: format!("chain-agent-{suffix}"),
            created_by: Some(CreationProvenance::system()),
            harness_kind: Some(crate::entities::harness::HarnessKind::AiAgents),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let channel = ChannelPersistence::create(
        persistence,
        company.id,
        ChannelWrite {
            name: "Chain".into(),
            slug: "chain".into(),
            agent_ids: Some(vec![agent.id]),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    (company, channel)
}

/// Move a chain's tasks back beyond the board's window.
///
/// Only `updated_at` moves. The status-event trigger fires on `status`, so the rows end up
/// looking exactly like work that has genuinely been idle for a fortnight, with no ledger
/// entry claiming something happened to them.
async fn age_chain_tasks(pool: &sqlx::PgPool, correlation_id: CorrelationId) {
    sqlx::query("UPDATE background_tasks SET updated_at = $1 WHERE correlation_id = $2")
        .bind(Utc::now() - chrono::Duration::days(30))
        .bind(correlation_id.as_uuid())
        .execute(pool)
        .await
        .unwrap();
}

async fn age_chain_deliveries(pool: &sqlx::PgPool, correlation_id: CorrelationId) {
    sqlx::query("UPDATE message_deliveries SET updated_at = $1 WHERE correlation_id = $2")
        .bind(Utc::now() - chrono::Duration::days(30))
        .bind(correlation_id.as_uuid())
        .execute(pool)
        .await
        .unwrap();
}

/// The board's cards as one chain-selection produces them, in an order two runs can be
/// compared in.
async fn board_cards(
    pool: &sqlx::PgPool,
    sql: &str,
    company_id: Uuid,
    filter: TaskBoardFilter,
    visible_channel_ids: &[Uuid],
) -> Vec<TaskChainCard> {
    let mut cards = sqlx::query_as::<_, TaskChainCardDb>(sql)
        .bind(company_id)
        .bind(filter.channel_id)
        .bind(filter.terminal_since)
        .bind(filter.per_column_limit as i64)
        .bind(visible_channel_ids)
        .fetch_all(pool)
        .await
        .unwrap()
        .into_iter()
        .map(|row| <(TaskChainCard, i64)>::try_from(row).unwrap().0)
        .collect::<Vec<_>>();
    cards.sort_by_key(|card| card.correlation_id.as_uuid());
    cards
}

/// One chain-starting task, on a thread of its own.
///
/// The thread is real rather than `None`: a delivery names the canonical message it carries, an
/// approval writes its request into the conversation it concerns, and both need somewhere to live.
async fn enqueue_chain(
    persistence: &PostgresPersistence,
    company_id: Uuid,
    channel_id: Uuid,
    task_type: &str,
) -> BackgroundTask {
    let thread = ThreadPersistence::create_thread(
        persistence,
        channel_id,
        &format!("{task_type} chain"),
        &[],
    )
    .await
    .unwrap();
    persistence
        .enqueue_task(NewTask::starting_new_chain(
            company_id,
            channel_id,
            Some(thread.id),
            task_type,
            serde_json::json!({}),
        ))
        .await
        .unwrap()
}

/// Take the lease a fenced write needs, the way the worker does.
/// Claim on behalf of a named worker, so a test can assert which one the ledger recorded.
async fn claim_as(
    persistence: &PostgresPersistence,
    task_id: Uuid,
    worker_id: Uuid,
) -> TaskLeaseRef {
    assert!(
        persistence
            .claim_task(
                task_id,
                worker_id,
                Utc::now() + chrono::Duration::minutes(5)
            )
            .await
            .unwrap()
    );
    let claimed = persistence.get_task_by_id(task_id).await.unwrap().unwrap();
    TaskLeaseRef::of(&claimed).unwrap()
}

/// Park a leased task behind an approval, returning the approval that now owns its state.
async fn park_for_approval(
    persistence: &PostgresPersistence,
    company: &crate::entities::company::Company,
    channel: &crate::entities::channel::Channel,
    thread_id: Uuid,
    lease: TaskLeaseRef,
) -> Uuid {
    use crate::entities::approval::{ApprovalAction, ApprovalSubject};
    use crate::entities::task::TaskSuspension;
    use crate::use_cases::approval::{ApprovalPersistence, NewApproval};

    let step_key = format!("step-{}", Uuid::new_v4().simple());
    let subject = ApprovalSubject {
        company_id: company.id,
        channel_id: channel.id,
        channel_name: channel.name.clone(),
        channel_slug: channel.slug.clone(),
        company_slug: company.slug.clone(),
        thread_id,
        suspension: Some(TaskSuspension::Leased(lease)),
        correlation_id: CorrelationId::new(),
        approver_email: "approver@example.com".into(),
    };
    // The request is a message in the thread and a delivery carrying it, written by the same
    // transaction that parks the task -- so an approval nobody could be told about is not a state
    // this can reach.
    let queued = delivery_fixture(
        persistence,
        DeliveryFixtureRequest {
            recipient: "approver@example.com",
            subject: "Approve",
            body: "Please approve",
            purpose: DeliveryPurpose::Notification,
            ..DeliveryFixtureRequest::new(company.id, channel.id, thread_id, &step_key)
        },
    )
    .await;
    let notice = MessageWrite::internal(
        thread_id,
        MessageAuthorWrite::Platform,
        "Approve",
        "Please approve",
        MessageDirection::Outbound,
        MessageRole::System,
        subject.correlation_id,
    )
    .external_conversation()
    .with_entry_kind(crate::entities::message::ThreadEntryKind::SystemEvent);
    let delivery = NewDelivery {
        message_id: notice.id,
        ..queued.delivery
    };

    let (approval, created) = persistence
        .create_approval(NewApproval {
            invocation: None,
            subject: &subject,
            action: &ApprovalAction {
                step_key,
                action_type: "generic".into(),
                title: "Approve".into(),
                summary: "Please approve".into(),
                payload: serde_json::json!({}),
            },
            message: &notice,
            delivery,
            token: Uuid::new_v4(),
            expires_at: Utc::now() + chrono::Duration::hours(24),
        })
        .await
        .unwrap();
    assert!(created, "the approval is new");
    approval.id
}

async fn claim(persistence: &PostgresPersistence, task_id: Uuid) -> TaskLeaseRef {
    assert!(
        persistence
            .claim_task(
                task_id,
                Uuid::new_v4(),
                Utc::now() + chrono::Duration::minutes(5)
            )
            .await
            .unwrap()
    );
    let claimed = persistence.get_task_by_id(task_id).await.unwrap().unwrap();
    TaskLeaseRef::of(&claimed).unwrap()
}

#[tokio::test]
async fn board_window_pushdown_selects_the_same_chains_as_the_aggregate_filter() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let (company, channel) = seed_company_and_channel(&persistence).await;

    // Every branch the board's selection has, each on its own chain.
    let active = enqueue_chain(&persistence, company.id, channel.id, "active").await;

    let unresolved = enqueue_chain(&persistence, company.id, channel.id, "unresolved").await;
    let lease = claim(&persistence, unresolved.id).await;
    persistence
        .mark_task_failed(TaskFailure {
            lease,
            error: "gave up",
            next_run_at: Utc::now(),
            outcome: TaskFailureOutcome::DeadLetter,
            reason: TaskStopReason::TerminalFailure,
        })
        .await
        .unwrap();
    // Unresolved work is selected however old it is — that is the point of the status arm.
    age_chain_tasks(&pool, unresolved.correlation_id).await;

    let stopped_recent = enqueue_chain(&persistence, company.id, channel.id, "stopped-in").await;
    persistence
        .stop_task(stopped_recent.id, StopActor::Operator(Uuid::new_v4()))
        .await
        .unwrap();

    let stopped_old = enqueue_chain(&persistence, company.id, channel.id, "stopped-out").await;
    persistence
        .stop_task(stopped_old.id, StopActor::Operator(Uuid::new_v4()))
        .await
        .unwrap();
    age_chain_tasks(&pool, stopped_old.correlation_id).await;

    let completed_recent =
        enqueue_chain(&persistence, company.id, channel.id, "completed-in").await;
    let lease = claim(&persistence, completed_recent.id).await;
    persistence.mark_task_completed(lease).await.unwrap();

    let completed_old = enqueue_chain(&persistence, company.id, channel.id, "completed-out").await;
    let lease = claim(&persistence, completed_old.id).await;
    persistence.mark_task_completed(lease).await.unwrap();
    age_chain_tasks(&pool, completed_old.correlation_id).await;

    // Finished, aged-out work whose only recent trace is a delivery. The pre-pushdown query
    // caught this one only incidentally, through the aggregate.
    let delivery_recent = enqueue_chain(&persistence, company.id, channel.id, "delivery-in").await;
    let lease = claim(&persistence, delivery_recent.id).await;
    persistence.mark_task_completed(lease).await.unwrap();
    let delivered_recently =
        queue_one_delivery(&persistence, &delivery_recent, "delivery-in").await;
    mark_delivered(&pool, delivered_recently).await;
    age_chain_tasks(&pool, delivery_recent.correlation_id).await;

    // The control for that arm: same shape, but the delivery is old too.
    let delivery_old = enqueue_chain(&persistence, company.id, channel.id, "delivery-out").await;
    let lease = claim(&persistence, delivery_old.id).await;
    persistence.mark_task_completed(lease).await.unwrap();
    let delivered_long_ago = queue_one_delivery(&persistence, &delivery_old, "delivery-out").await;
    mark_delivered(&pool, delivered_long_ago).await;
    age_chain_tasks(&pool, delivery_old.correlation_id).await;
    age_chain_deliveries(&pool, delivery_old.correlation_id).await;

    let filter = TaskBoardFilter::new(None, Utc::now());
    let pushdown = board_cards(&pool, &BOARD_QUERY, company.id, filter, &[channel.id]).await;
    let control = board_cards(
        &pool,
        &board_query_sql(BOARD_ELIGIBLE_EVERY_CHAIN),
        company.id,
        filter,
        &[channel.id],
    )
    .await;
    assert_eq!(
        pushdown, control,
        "the row-level selection and the aggregate filter must pick the same chains"
    );

    // Agreement alone would also be satisfied by both being wrong, so pin the membership too.
    let mut selected = pushdown
        .iter()
        .map(|card| card.correlation_id)
        .collect::<Vec<_>>();
    selected.sort_by_key(|id| id.as_uuid());
    let mut expected = vec![
        active.correlation_id,
        unresolved.correlation_id,
        stopped_recent.correlation_id,
        completed_recent.correlation_id,
        delivery_recent.correlation_id,
    ];
    expected.sort_by_key(|id| id.as_uuid());
    assert_eq!(selected, expected);
    assert!(!selected.contains(&stopped_old.correlation_id));
    assert!(!selected.contains(&completed_old.correlation_id));
    assert!(!selected.contains(&delivery_old.correlation_id));

    // The channel filter is applied on top of the row-level selection, not instead of it.
    let other_channel = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "Elsewhere".into(),
            slug: "elsewhere".into(),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let filtered = TaskBoardFilter::new(Some(other_channel.id), Utc::now());
    assert!(
        board_cards(
            &pool,
            &BOARD_QUERY,
            company.id,
            filtered,
            &[other_channel.id],
        )
        .await
        .is_empty()
    );

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// Every count field on its own, then the crossings where precedence is what actually
/// decides. One query per case rather than one batched query: a mismatch has to be able to
/// name the counts that produced it.
fn stage_matrix() -> Vec<TaskChainCounts> {
    vec![
        // Nothing at all: no arm matches, so both sides must fall to the `ELSE`.
        TaskChainCounts::default(),
        // Each field alone, so no arm is only ever reached alongside another.
        TaskChainCounts {
            total_tasks: 1,
            ..Default::default()
        },
        TaskChainCounts {
            pending: 1,
            ..Default::default()
        },
        TaskChainCounts {
            processing: 1,
            ..Default::default()
        },
        TaskChainCounts {
            expired_processing: 1,
            ..Default::default()
        },
        TaskChainCounts {
            pending_approval: 1,
            ..Default::default()
        },
        TaskChainCounts {
            waiting_reply: 1,
            ..Default::default()
        },
        TaskChainCounts {
            completed: 1,
            ..Default::default()
        },
        TaskChainCounts {
            failed: 1,
            ..Default::default()
        },
        TaskChainCounts {
            dead_letter: 1,
            ..Default::default()
        },
        TaskChainCounts {
            stopped: 1,
            ..Default::default()
        },
        TaskChainCounts {
            total_deliveries: 1,
            ..Default::default()
        },
        TaskChainCounts {
            delivery_queued: 1,
            ..Default::default()
        },
        TaskChainCounts {
            delivery_sending: 1,
            ..Default::default()
        },
        TaskChainCounts {
            delivery_delivered: 1,
            ..Default::default()
        },
        TaskChainCounts {
            delivery_unresolved: 1,
            ..Default::default()
        },
        // Needs-attention outranks everything below it, by each of its five triggers.
        TaskChainCounts {
            total_tasks: 4,
            failed: 1,
            pending_approval: 1,
            processing: 1,
            pending: 1,
            ..Default::default()
        },
        TaskChainCounts {
            total_tasks: 2,
            dead_letter: 1,
            waiting_reply: 1,
            ..Default::default()
        },
        TaskChainCounts {
            total_tasks: 2,
            stopped: 1,
            pending: 1,
            ..Default::default()
        },
        TaskChainCounts {
            total_tasks: 1,
            expired_processing: 1,
            processing: 1,
            ..Default::default()
        },
        TaskChainCounts {
            total_tasks: 1,
            completed: 1,
            total_deliveries: 1,
            delivery_unresolved: 1,
            pending_approval: 1,
            ..Default::default()
        },
        // Then each remaining rung against the ones it outranks.
        TaskChainCounts {
            total_tasks: 4,
            pending_approval: 1,
            processing: 1,
            waiting_reply: 1,
            pending: 1,
            ..Default::default()
        },
        TaskChainCounts {
            total_tasks: 3,
            processing: 1,
            waiting_reply: 1,
            pending: 1,
            ..Default::default()
        },
        TaskChainCounts {
            total_tasks: 1,
            waiting_reply: 1,
            total_deliveries: 1,
            delivery_sending: 1,
            ..Default::default()
        },
        TaskChainCounts {
            total_tasks: 2,
            waiting_reply: 1,
            pending: 1,
            ..Default::default()
        },
        TaskChainCounts {
            total_tasks: 1,
            completed: 1,
            total_deliveries: 1,
            delivery_queued: 1,
            ..Default::default()
        },
        // Completed needs every task *and* every delivery to have landed.
        TaskChainCounts {
            total_tasks: 2,
            completed: 2,
            total_deliveries: 1,
            delivery_delivered: 1,
            ..Default::default()
        },
        TaskChainCounts {
            total_tasks: 1,
            completed: 1,
            ..Default::default()
        },
        // ...and short of that, with nothing live left, both sides fall through to `ELSE`.
        TaskChainCounts {
            total_tasks: 2,
            completed: 1,
            ..Default::default()
        },
        TaskChainCounts {
            total_tasks: 1,
            completed: 1,
            total_deliveries: 2,
            delivery_delivered: 1,
            ..Default::default()
        },
    ]
}

/// Ask Postgres for the stage the board would assign one set of counts, using the board's own
/// expression rather than a paraphrase of it.
async fn stage_from_sql(pool: &sqlx::PgPool, counts: &TaskChainCounts) -> ChainStage {
    let stage: String = sqlx::query_scalar(&format!(
        "SELECT {CHAIN_STAGE_SQL_CASE} FROM (
                 SELECT $1::bigint AS total_tasks, $2::bigint AS pending,
                        $3::bigint AS processing, $4::bigint AS expired_processing,
                        $5::bigint AS pending_approval, $6::bigint AS waiting_reply,
                        $7::bigint AS completed, $8::bigint AS failed,
                        $9::bigint AS dead_letter, $10::bigint AS stopped,
                        $11::bigint AS total_deliveries, $12::bigint AS delivery_queued,
                        $13::bigint AS delivery_sending, $14::bigint AS delivery_delivered,
                        $15::bigint AS delivery_unresolved
             ) AS combined"
    ))
    .bind(counts.total_tasks)
    .bind(counts.pending)
    .bind(counts.processing)
    .bind(counts.expired_processing)
    .bind(counts.pending_approval)
    .bind(counts.waiting_reply)
    .bind(counts.completed)
    .bind(counts.failed)
    .bind(counts.dead_letter)
    .bind(counts.stopped)
    .bind(counts.total_deliveries)
    .bind(counts.delivery_queued)
    .bind(counts.delivery_sending)
    .bind(counts.delivery_delivered)
    .bind(counts.delivery_unresolved)
    .fetch_one(pool)
    .await
    .unwrap();
    ChainStage::from_str(&stage).unwrap()
}

/// The release-build guard on the two stage representations.
///
/// `ChainStage::derive` and `CHAIN_STAGE_SQL_CASE` are one rule written twice, because the
/// board needs `stage` as a column to partition on and the domain may not carry SQL. Nothing
/// in a release build compared them: the conversion carried a `debug_assert_eq!`, which only
/// fired in debug builds and then panicked on whatever a production row happened to hold
/// rather than on the drift itself. This pushes a matrix of counts through the real
/// expression instead, so a rung edited on one side fails here.
#[tokio::test]
async fn chain_stage_sql_matches_rust_derivation() {
    let Some(pool) = test_pool().await else {
        return;
    };
    for counts in stage_matrix() {
        assert_eq!(
            stage_from_sql(&pool, &counts).await,
            ChainStage::derive(&counts),
            "SQL and Rust disagree for {counts:?}"
        );
    }
}

/// One queued delivery for a task, carrying a message in its own thread.
async fn queue_one_delivery(
    persistence: &PostgresPersistence,
    task: &BackgroundTask,
    key: &str,
) -> Uuid {
    let thread_id = task
        .thread_id
        .expect("a chain fixture task runs on a thread");
    let queued = delivery_fixture(
        persistence,
        DeliveryFixtureRequest {
            task_id: Some(task.id),
            ..DeliveryFixtureRequest::new(
                task.company_id,
                task.channel_id,
                thread_id,
                &format!("{key}-{}", Uuid::new_v4()),
            )
        },
    )
    .await;
    let id = queued.delivery.id;
    persistence.enqueue_delivery(queued.delivery).await.unwrap();
    // The chain rollups read `correlation_id`, and a fixture that minted its own would sort into a
    // chain of one.
    sqlx::query("UPDATE message_deliveries SET correlation_id = $2 WHERE id = $1")
        .bind(id.as_uuid())
        .bind(task.correlation_id.as_uuid())
        .execute(&persistence.pool)
        .await
        .unwrap();
    id.as_uuid()
}

/// Land one queued delivery as delivered.
///
/// Written directly rather than through `claim_deliveries`, which claims from the whole table:
/// these tests share a database with everything else running in parallel, and a claim would take
/// rows they do not own.
async fn mark_delivered(pool: &sqlx::PgPool, delivery_id: Uuid) {
    sqlx::query(
        "UPDATE message_delivery_parts
            SET status = 'delivered', request_started_at = CURRENT_TIMESTAMP,
                delivered_at = CURRENT_TIMESTAMP
          WHERE delivery_id = $1",
    )
    .bind(delivery_id)
    .execute(pool)
    .await
    .unwrap();
    let updated = sqlx::query(
        "UPDATE message_deliveries
                SET status = 'delivered', delivered_at = CURRENT_TIMESTAMP,
                    updated_at = CURRENT_TIMESTAMP
              WHERE id = $1",
    )
    .bind(delivery_id)
    .execute(pool)
    .await
    .unwrap();
    assert_eq!(updated.rows_affected(), 1);
}

#[tokio::test]
async fn chain_detail_attaches_every_attempt_and_delivery_to_its_own_task() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company, channel) = seed_company_and_channel(&persistence).await;

    let first = enqueue_chain(&persistence, company.id, channel.id, "grouped-first").await;
    let mut tasks = vec![first.clone()];
    for index in 1..3 {
        let thread = ThreadPersistence::create_thread(
            &persistence,
            channel.id,
            &format!("grouped-{index}"),
            &[],
        )
        .await
        .unwrap();
        tasks.push(
            persistence
                .enqueue_task(NewTask {
                    targets: Vec::new(),
                    source: TaskSource::Unattributed,
                    company_id: company.id,
                    channel_id: channel.id,
                    thread_id: Some(thread.id),
                    task_type: format!("grouped-{index}"),
                    payload: serde_json::json!({}),
                    correlation_id: first.correlation_id,
                })
                .await
                .unwrap(),
        );
    }

    // Task n gets n attempts and n deliveries, so a grouping that loses the association shows
    // up as a wrong count rather than only as a wrong order.
    for (index, task) in tasks.iter().enumerate() {
        let expected = index + 1;
        for attempt_number in 1..=expected as i32 {
            let attempt = TaskAttemptRef {
                task_id: task.id,
                attempt_number,
                execution_generation: Uuid::new_v4(),
                worker_id: Uuid::new_v4(),
            };
            persistence
                .begin_task_attempt(attempt, &test_machine())
                .await
                .unwrap();
        }
        for _ in 0..expected {
            queue_one_delivery(&persistence, task, "grouped").await;
        }
    }

    let detail = persistence
        .get_task_chain_detail(
            CollaborationReadScope {
                company_id: company.id,
                visible_channel_ids: &[channel.id],
            },
            first.correlation_id,
        )
        .await
        .unwrap()
        .unwrap();
    assert!(!detail.truncated);
    assert_eq!(detail.tasks.len(), 3);
    for (index, item) in detail.tasks.iter().enumerate() {
        let expected = index + 1;
        assert_eq!(item.task.id, tasks[index].id);
        assert_eq!(item.attempts.len(), expected, "attempts for task {index}");
        assert_eq!(
            item.deliveries.len(),
            expected,
            "deliveries for task {index}"
        );
        assert!(
            item.deliveries
                .iter()
                .all(|delivery| delivery.task_id == Some(item.task.id))
        );
        assert_eq!(
            item.attempts
                .iter()
                .map(|attempt| attempt.attempt_number)
                .collect::<Vec<_>>(),
            (1..=expected as i32).collect::<Vec<_>>()
        );
    }

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// One chain with `count` extra tasks alongside the one it starts from.
///
/// Bulk-inserted in a single statement: these fixtures cross limits in the low thousands, and
/// a round trip per row would make the test slower than everything else in the suite put
/// together.
async fn bulk_tasks(pool: &sqlx::PgPool, task: &BackgroundTask, count: i32) {
    sqlx::query(
        "INSERT INTO background_tasks (id, company_id, channel_id, correlation_id, task_type)
             SELECT gen_random_uuid(), $1, $2, $3, 'bulk-' || series
               FROM generate_series(1, $4::int) AS series",
    )
    .bind(task.company_id)
    .bind(task.channel_id)
    .bind(task.correlation_id.as_uuid())
    .bind(count)
    .execute(pool)
    .await
    .unwrap();
}

async fn bulk_attempts(pool: &sqlx::PgPool, task_id: Uuid, count: i32) {
    sqlx::query(
        "INSERT INTO task_attempts
                 (id, task_id, attempt_number, status, execution_generation,
                  worker_id, machine_id, machine_region)
             SELECT gen_random_uuid(), $1, series, 'completed', gen_random_uuid(),
                    gen_random_uuid(), 'bulk-machine', NULL
               FROM generate_series(1, $2::int) AS series",
    )
    .bind(task_id)
    .bind(count)
    .execute(pool)
    .await
    .unwrap();
}

/// Many delivered rows for one task, cloned off a real one.
///
/// The clone is what keeps the foreign keys satisfiable without building a message and an
/// interface per row: every copy names the same canonical message and the same binding, which is
/// exactly what a chain of mirrors would look like anyway.
async fn bulk_deliveries(pool: &sqlx::PgPool, task: &BackgroundTask, template: Uuid, count: i32) {
    sqlx::query(
        "INSERT INTO message_deliveries
                 (id, company_id, channel_id, message_id, source_binding_id,
                  destination_binding_id, external_destination, task_id, correlation_id,
                  transport, purpose, idempotency_key, status, max_attempts, delivered_at)
             SELECT gen_random_uuid(), template.company_id, template.channel_id,
                    template.message_id, template.source_binding_id,
                    template.destination_binding_id, template.external_destination,
                    template.task_id, template.correlation_id, template.transport,
                    template.purpose, template.idempotency_key || '-' || series,
                    'delivered', template.max_attempts, CURRENT_TIMESTAMP
               FROM message_deliveries AS template, generate_series(1, $2::int) AS series
              WHERE template.id = $1",
    )
    .bind(template)
    .bind(count)
    .execute(pool)
    .await
    .unwrap();
    let _ = task;
}

/// Extra ledger rows past the one the enqueue already wrote, hence the sequence offset.
async fn bulk_status_events(pool: &sqlx::PgPool, task: &BackgroundTask, count: i32) {
    sqlx::query(
        "INSERT INTO task_status_events
                 (id, company_id, task_id, correlation_id, sequence, to_status, reason,
                  actor_kind, retry_count, run_at)
             SELECT gen_random_uuid(), $1, $2, $3, series + 100, 'pending', 'enqueued',
                    'system', 0, CURRENT_TIMESTAMP
               FROM generate_series(1, $4::int) AS series",
    )
    .bind(task.company_id)
    .bind(task.id)
    .bind(task.correlation_id.as_uuid())
    .bind(count)
    .execute(pool)
    .await
    .unwrap();
}

async fn bulk_approvals(pool: &sqlx::PgPool, task: &BackgroundTask, count: i32) {
    sqlx::query(
        "INSERT INTO human_approvals
                 (id, company_id, channel_id, thread_id, task_id, step_key, approver_email,
                  action_type, action_title, action_summary, token, expires_at)
             SELECT gen_random_uuid(), $1, $2, $3, $4, $5 || '-' || series,
                    'approver@example.com', 'send', 'Bulk approval', 'Bulk summary',
                    gen_random_uuid(), CURRENT_TIMESTAMP + interval '1 day'
               FROM generate_series(1, $6::int) AS series",
    )
    .bind(task.company_id)
    .bind(task.channel_id)
    .bind(
        task.thread_id
            .expect("an approval belongs to the conversation it concerns"),
    )
    .bind(task.id)
    .bind(Uuid::new_v4().to_string())
    .bind(count)
    .execute(pool)
    .await
    .unwrap();
}

async fn bulk_outreaches(pool: &sqlx::PgPool, task_id: Uuid, count: i32) {
    sqlx::query(
        "INSERT INTO task_outreaches
                 (id, task_id, company_id, status, required_threshold_percent, expires_at,
                  outreach_key, subject, body)
             SELECT gen_random_uuid(), task.id, task.company_id, 'waiting', 50,
                    CURRENT_TIMESTAMP + interval '1 day', $2 || '-' || series, 'Subject', 'Body'
               FROM background_tasks AS task
               CROSS JOIN generate_series(1, $3::int) AS series
              WHERE task.id = $1",
    )
    .bind(task_id)
    .bind(Uuid::new_v4().to_string())
    .bind(count)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn chain_detail_bounds_every_collection_and_reports_the_truncation() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let detail_of = async |correlation_id| {
        persistence
            .get_task_chain_detail(
                CollaborationReadScope {
                    company_id: company.id,
                    visible_channel_ids: &[channel.id],
                },
                correlation_id,
            )
            .await
            .unwrap()
            .unwrap()
    };

    // Tasks. The 201st task also pushes the chain past the event limit, since every enqueue
    // writes one ledger row — which is exactly why `truncated` is one flag and not six.
    let many_tasks = enqueue_chain(&persistence, company.id, channel.id, "limit-tasks").await;
    bulk_tasks(&pool, &many_tasks, CHAIN_DETAIL_MAX_TASKS as i32).await;
    let detail = detail_of(many_tasks.correlation_id).await;
    assert!(detail.truncated);
    assert_eq!(detail.tasks.len(), CHAIN_DETAIL_MAX_TASKS as usize);

    // Attempts, one past the limit.
    let many_attempts = enqueue_chain(&persistence, company.id, channel.id, "limit-attempts").await;
    bulk_attempts(
        &pool,
        many_attempts.id,
        CHAIN_DETAIL_MAX_ATTEMPTS as i32 + 1,
    )
    .await;
    let detail = detail_of(many_attempts.correlation_id).await;
    assert!(detail.truncated);
    assert_eq!(
        detail.tasks[0].attempts.len(),
        CHAIN_DETAIL_MAX_ATTEMPTS as usize
    );

    // Attempts, exactly at the limit. A full page is not a truncated one, and only the
    // sentinel row can tell the two apart.
    let full_attempts =
        enqueue_chain(&persistence, company.id, channel.id, "limit-attempts-exact").await;
    bulk_attempts(&pool, full_attempts.id, CHAIN_DETAIL_MAX_ATTEMPTS as i32).await;
    let detail = detail_of(full_attempts.correlation_id).await;
    assert!(!detail.truncated);
    assert_eq!(
        detail.tasks[0].attempts.len(),
        CHAIN_DETAIL_MAX_ATTEMPTS as usize
    );

    // Deliveries.
    let many_deliveries =
        enqueue_chain(&persistence, company.id, channel.id, "limit-deliveries").await;
    let template = queue_one_delivery(&persistence, &many_deliveries, "limit-deliveries").await;
    bulk_deliveries(
        &pool,
        &many_deliveries,
        template,
        CHAIN_DETAIL_MAX_DELIVERIES as i32,
    )
    .await;
    let detail = detail_of(many_deliveries.correlation_id).await;
    assert!(detail.truncated);
    assert_eq!(
        detail.tasks[0].deliveries.len(),
        CHAIN_DETAIL_MAX_DELIVERIES as usize
    );

    // Events: the enqueue wrote one, so the limit is crossed by the limit's worth on top.
    let many_events = enqueue_chain(&persistence, company.id, channel.id, "limit-events").await;
    bulk_status_events(&pool, &many_events, CHAIN_DETAIL_MAX_EVENTS as i32).await;
    let detail = detail_of(many_events.correlation_id).await;
    assert!(detail.truncated);
    assert_eq!(detail.events.len(), CHAIN_DETAIL_MAX_EVENTS as usize);

    // Approvals.
    let many_approvals =
        enqueue_chain(&persistence, company.id, channel.id, "limit-approvals").await;
    bulk_approvals(
        &pool,
        &many_approvals,
        CHAIN_DETAIL_MAX_APPROVALS as i32 + 1,
    )
    .await;
    let detail = detail_of(many_approvals.correlation_id).await;
    assert!(detail.truncated);
    assert_eq!(detail.approvals.len(), CHAIN_DETAIL_MAX_APPROVALS as usize);

    // Outreaches.
    let many_outreaches =
        enqueue_chain(&persistence, company.id, channel.id, "limit-outreaches").await;
    bulk_outreaches(
        &pool,
        many_outreaches.id,
        CHAIN_DETAIL_MAX_OUTREACHES as i32 + 1,
    )
    .await;
    let detail = detail_of(many_outreaches.correlation_id).await;
    assert!(detail.truncated);
    assert_eq!(
        detail.outreaches.len(),
        CHAIN_DETAIL_MAX_OUTREACHES as usize
    );

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

#[test]
fn a_full_page_is_not_a_truncated_one() {
    let mut exactly_full = (0..3).collect::<Vec<_>>();
    assert!(!trim_to_limit(&mut exactly_full, 3));
    assert_eq!(exactly_full.len(), 3);

    let mut one_over = (0..probe_limit(3)).collect::<Vec<_>>();
    assert!(trim_to_limit(&mut one_over, 3));
    assert_eq!(one_over, vec![0, 1, 2]);
}

#[tokio::test]
async fn a_status_write_that_changes_nothing_wakes_no_board() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let (company, channel) = seed_company_and_channel(&persistence).await;

    // Everything is in place before the listener attaches, so the only notifications it can
    // see are the ones this test provokes.
    let quiet = enqueue_chain(&persistence, company.id, channel.id, "quiet").await;
    let delivery_id = queue_one_delivery(&persistence, &quiet, "quiet").await;
    bulk_approvals(&pool, &quiet, 1).await;
    bulk_outreaches(&pool, quiet.id, 1).await;
    let fence = enqueue_chain(&persistence, company.id, channel.id, "fence").await;
    let fence_delivery = queue_one_delivery(&persistence, &fence, "fence").await;

    let mut listener = sqlx::postgres::PgListener::connect_with(&pool)
        .await
        .unwrap();
    listener.listen("task_chain_changed").await.unwrap();

    // `UPDATE OF status` fires whenever the column is in the SET list, value unchanged or not.
    for statement in [
        "UPDATE message_deliveries SET status = status, updated_at = CURRENT_TIMESTAMP
              WHERE id = $1",
        "UPDATE human_approvals SET status = status, updated_at = CURRENT_TIMESTAMP
              WHERE task_id = $1",
        "UPDATE task_outreaches SET status = status, updated_at = CURRENT_TIMESTAMP
              WHERE task_id = $1",
    ] {
        let target = if statement.contains("message_deliveries") {
            delivery_id
        } else {
            quiet.id
        };
        let affected = sqlx::query(statement)
            .bind(target)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(affected.rows_affected(), 1, "no-op write must hit its row");
    }

    // A real change on another chain, committed after all three no-ops. Notifications arrive
    // in commit order on one connection, so reaching this one means the no-ops sent nothing.
    mark_delivered(&pool, fence_delivery).await;

    let quiet_wakes = drain_chain_notifications(&mut listener, company.id, fence.correlation_id)
        .await
        .into_iter()
        .filter(|id| *id == quiet.correlation_id.as_uuid())
        .count();
    assert_eq!(
        quiet_wakes, 0,
        "no-op status writes must not wake the board"
    );

    // The same three tables, actually changing status, still do wake it.
    mark_delivered(&pool, delivery_id).await;
    for statement in [
        "UPDATE human_approvals SET status = 'approved' WHERE task_id = $1",
        "UPDATE task_outreaches SET status = 'threshold_met' WHERE task_id = $1",
    ] {
        sqlx::query(statement)
            .bind(quiet.id)
            .execute(&pool)
            .await
            .unwrap();
    }
    mark_delivered(
        &pool,
        queue_one_delivery(&persistence, &fence, "fence-2").await,
    )
    .await;
    let real_wakes = drain_chain_notifications(&mut listener, company.id, fence.correlation_id)
        .await
        .into_iter()
        .filter(|id| *id == quiet.correlation_id.as_uuid())
        .count();
    assert_eq!(
        real_wakes, 3,
        "delivery, approval and outreach each moved status"
    );

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// Every chain notification this company emitted up to and including `until`.
///
/// The fence is what makes a negative assertion possible: waiting for a notification that
/// should never arrive can only ever be a timeout, whereas waiting for one that must arrive
/// after it proves the earlier writes had their chance.
async fn drain_chain_notifications(
    listener: &mut sqlx::postgres::PgListener,
    company_id: Uuid,
    until: CorrelationId,
) -> Vec<Uuid> {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut seen = Vec::new();
        loop {
            let notification = listener.recv().await.unwrap();
            let payload: serde_json::Value = serde_json::from_str(notification.payload()).unwrap();
            if payload["company_id"] != company_id.to_string() {
                continue;
            }
            let correlation_id =
                Uuid::parse_str(payload["correlation_id"].as_str().unwrap()).unwrap();
            if correlation_id == until.as_uuid() {
                return seen;
            }
            seen.push(correlation_id);
        }
    })
    .await
    .expect("the fencing notification arrives")
}

#[tokio::test]
async fn task_chain_notifications_contain_identifiers_only() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let mut listener = sqlx::postgres::PgListener::connect_with(&pool)
        .await
        .unwrap();
    listener.listen("task_chain_changed").await.unwrap();

    let task = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            channel.id,
            None,
            "notification-test",
            serde_json::json!({"body": "must never be notified", "token": "secret"}),
        ))
        .await
        .unwrap();
    let payload = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let notification = listener.recv().await.unwrap();
            let payload: serde_json::Value = serde_json::from_str(notification.payload()).unwrap();
            if payload["company_id"] == company.id.to_string()
                && payload["correlation_id"] == task.correlation_id.to_string()
            {
                break payload;
            }
        }
    })
    .await
    .expect("task-chain notification arrives");
    assert_eq!(payload.as_object().unwrap().len(), 2);
    assert_eq!(payload["company_id"], company.id.to_string());
    assert_eq!(payload["correlation_id"], task.correlation_id.to_string());
    assert!(payload.get("body").is_none());
    assert!(payload.get("token").is_none());

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// The sweep names the worker that lost each lease, without the trigger reading `last_error`.
///
/// The trigger used to recognise lease loss by comparing `NEW.last_error` against a copy of
/// `LEASE_EXPIRED_ERROR`, so editing that Rust constant would have silently refiled every
/// later lease loss as `retryable_failure`. Those arms are gone; this is what proves deleting
/// them was safe. The sweep is one statement over rows held by different workers, so the
/// per-row attribution is the part worth pinning: `transition_actor_id = worker_id` is read
/// from the old row version, and each event must therefore carry *its own* worker.
#[tokio::test]
async fn a_reaped_lease_records_lease_lost_against_the_worker_that_held_it() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let (company, channel) = seed_company_and_channel(&persistence).await;

    // Two chains leased by two different workers, expiring in the same sweep: a shared batch
    // actor would pass with one and fail here.
    let mut leased = Vec::new();
    for _ in 0..2 {
        let task = enqueue_chain(&persistence, company.id, channel.id, "reaped").await;
        let worker_id = Uuid::new_v4();
        claim_as(&persistence, task.id, worker_id).await;
        leased.push((task, worker_id));
    }

    // The runs vanish: both leases lapse with nothing reported.
    for (task, _) in &leased {
        sqlx::query(
            "UPDATE background_tasks
                 SET locked_at = CURRENT_TIMESTAMP - interval '20 minutes',
                     lock_expires_at = CURRENT_TIMESTAMP - interval '1 second'
                 WHERE id = $1",
        )
        .bind(task.id)
        .execute(&pool)
        .await
        .unwrap();
    }

    // The sweep is global, so a test running beside this one may reap these rows first. What
    // matters is the ledger they end up with, not whose call got there.
    persistence.reap_expired_task_leases().await.unwrap();

    for (task, worker_id) in &leased {
        let events = persistence
            .list_task_status_events(company.id, task.correlation_id, None, 20)
            .await
            .unwrap();
        let lost = events
            .iter()
            .find(|event| event.reason == TaskTransitionReason::LeaseLost)
            .expect("a reaped lease records why the run ended");
        assert_eq!(lost.task_id, task.id);
        assert_eq!(lost.from_status, Some(TaskStatus::Processing));
        assert_eq!(lost.to_status, TaskStatus::Pending);
        assert_eq!(lost.actor_kind, TaskTransitionActorKind::Worker);
        assert_eq!(
            lost.actor_id,
            Some(*worker_id),
            "each event must name the worker whose lease that row lost"
        );
        assert_eq!(lost.related_approval_id, None);
        assert_eq!(lost.related_outreach_id, None);
        assert!(
            !events
                .iter()
                .any(|event| event.reason == TaskTransitionReason::RetryableFailure),
            "lease loss must not fall through to the retryable-failure arm"
        );
    }

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn worker_outcomes_record_exact_transition_reasons() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let cases = [
        (TaskStopReason::RetryableFailure, TaskFailureOutcome::Retry),
        (TaskStopReason::TimedOut, TaskFailureOutcome::Retry),
        (TaskStopReason::Shutdown, TaskFailureOutcome::Retry),
        (
            TaskStopReason::TerminalFailure,
            TaskFailureOutcome::DeadLetter,
        ),
    ];
    for (stop_reason, dead_letter) in cases {
        let task = persistence
            .enqueue_task(NewTask::starting_new_chain(
                company.id,
                channel.id,
                None,
                format!("reason-{}", stop_reason.as_str()),
                serde_json::json!({}),
            ))
            .await
            .unwrap();
        let worker_id = Uuid::new_v4();
        assert!(
            persistence
                .claim_task(
                    task.id,
                    worker_id,
                    Utc::now() + chrono::Duration::minutes(5),
                )
                .await
                .unwrap()
        );
        let claimed = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
        let lease = TaskLeaseRef::of(&claimed).unwrap();
        assert!(
            persistence
                .mark_task_failed(TaskFailure {
                    lease,
                    error: "bounded test failure",
                    next_run_at: Utc::now() + chrono::Duration::minutes(1),
                    outcome: dead_letter,
                    reason: stop_reason,
                })
                .await
                .unwrap()
        );
        let expected = match stop_reason {
            TaskStopReason::RetryableFailure => TaskTransitionReason::RetryableFailure,
            TaskStopReason::TimedOut => TaskTransitionReason::TimedOut,
            TaskStopReason::Shutdown => TaskTransitionReason::Shutdown,
            TaskStopReason::TerminalFailure => TaskTransitionReason::TerminalFailure,
            _ => unreachable!(),
        };
        let events = persistence
            .list_task_status_events(company.id, task.correlation_id, None, 20)
            .await
            .unwrap();
        assert!(events.iter().any(|event| event.reason == expected));
    }

    let completed = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            channel.id,
            None,
            "reason-completed",
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    let worker_id = Uuid::new_v4();
    persistence
        .claim_task(
            completed.id,
            worker_id,
            Utc::now() + chrono::Duration::minutes(5),
        )
        .await
        .unwrap();
    let claimed = persistence
        .get_task_by_id(completed.id)
        .await
        .unwrap()
        .unwrap();
    persistence
        .mark_task_completed(TaskLeaseRef::of(&claimed).unwrap())
        .await
        .unwrap();
    let events = persistence
        .list_task_status_events(company.id, completed.correlation_id, None, 20)
        .await
        .unwrap();
    assert!(
        events
            .iter()
            .any(|event| event.reason == TaskTransitionReason::Completed)
    );

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn task_attempt_history_is_ordered_and_company_scoped() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let (other_company, _) = seed_company_and_channel(&persistence).await;
    let task = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            channel.id,
            None,
            "attempt-history",
            serde_json::json!({}),
        ))
        .await
        .unwrap();

    // Distinct workers per attempt: a task retried across a restart runs somewhere else, and the
    // ledger is the only durable record of which run was which.
    let workers = [Uuid::new_v4(), Uuid::new_v4()];
    for attempt_number in [1, 2] {
        let attempt = TaskAttemptRef {
            task_id: task.id,
            attempt_number,
            execution_generation: Uuid::new_v4(),
            worker_id: workers[attempt_number as usize - 1],
        };
        persistence
            .begin_task_attempt(attempt, &test_machine())
            .await
            .unwrap();
        persistence
            .finish_task_attempt(&TaskAttemptOutcome {
                attempt,
                status: if attempt_number == 1 {
                    TaskAttemptStatus::Failed
                } else {
                    TaskAttemptStatus::Completed
                },
                stop_reason: if attempt_number == 1 {
                    TaskStopReason::RetryableFailure
                } else {
                    TaskStopReason::Completed
                },
                error: (attempt_number == 1).then(|| "retry me".to_string()),
                tokens: Some(TokenUsage::new(
                    attempt_number as usize * 10,
                    attempt_number as usize * 2,
                )),
            })
            .await
            .unwrap();
    }

    let attempts = persistence
        .list_task_attempts(company.id, task.id)
        .await
        .unwrap();
    assert_eq!(
        attempts
            .iter()
            .map(|attempt| attempt.attempt_number)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(attempts[0].status, TaskAttemptRecordStatus::Failed);
    assert_eq!(
        attempts[0].stop_reason,
        Some(TaskStopReason::RetryableFailure)
    );
    assert_eq!(attempts[0].total_tokens(), Some(12));
    assert!(attempts[0].duration_ms().is_some());
    assert_eq!(
        attempts
            .iter()
            .map(|attempt| attempt.worker_id)
            .collect::<Vec<_>>(),
        workers.to_vec(),
        "each attempt keeps the worker that made it, not the last one to run"
    );
    assert_eq!(attempts[0].machine, test_machine());
    assert!(
        persistence
            .list_task_attempts(other_company.id, task.id)
            .await
            .unwrap()
            .is_empty(),
        "another company cannot read a guessed task's attempt history"
    );

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
    CompanyPersistence::delete(&persistence, other_company.id)
        .await
        .unwrap();
}

/// The whole point of the correlation id: one query returns the trail.
#[tokio::test]
async fn a_chain_is_inherited_by_children_and_readable_in_one_query() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let (company, channel) = seed_company_and_channel(&persistence).await;

    // The inbound message mints one chain; a second, unrelated message mints another.
    let chain = CorrelationId::new();
    let other_chain = CorrelationId::new();

    let parent_thread = ThreadPersistence::create_thread(&persistence, channel.id, "Chain", &[])
        .await
        .unwrap();
    let parent = persistence
        .enqueue_task(NewTask {
            targets: Vec::new(),
            source: TaskSource::Unattributed,
            company_id: company.id,
            channel_id: channel.id,
            thread_id: Some(parent_thread.id),
            task_type: "email_agent_dispatch".to_string(),
            payload: serde_json::json!({}),
            correlation_id: chain,
        })
        .await
        .unwrap();
    assert_eq!(parent.correlation_id, chain, "the chain round-trips");

    // A task the run spawns -- an outreach into another channel, say -- inherits it.
    let child = persistence
        .enqueue_task(NewTask::caused_by(
            &parent,
            channel.id,
            None,
            "email_agent_dispatch",
            serde_json::json!({ "spawned": true }),
        ))
        .await
        .unwrap();
    assert_eq!(child.correlation_id, chain);

    let unrelated = persistence
        .enqueue_task(NewTask {
            targets: Vec::new(),
            source: TaskSource::Unattributed,
            company_id: company.id,
            channel_id: channel.id,
            thread_id: None,
            task_type: "email_agent_dispatch".to_string(),
            payload: serde_json::json!({ "unrelated": true }),
            correlation_id: other_chain,
        })
        .await
        .unwrap();

    // The delivery the run queues carries the chain too, so the outbound leg is on the trail.
    queue_one_delivery(&persistence, &parent, "chain-test").await;

    let task_ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM background_tasks WHERE correlation_id = $1 ORDER BY created_at",
    )
    .bind(chain.as_uuid())
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(task_ids.contains(&parent.id));
    assert!(task_ids.contains(&child.id));
    assert!(
        !task_ids.contains(&unrelated.id),
        "a different message must not join this chain"
    );

    let sent: i64 =
        sqlx::query_scalar("SELECT count(*) FROM message_deliveries WHERE correlation_id = $1")
            .bind(chain.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(sent, 1, "the outbound leg is on the same trail");
}

/// A redelivered message must not fork the chain its first delivery started.
#[tokio::test]
async fn a_redelivered_message_rejoins_its_original_chain() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company, channel) = seed_company_and_channel(&persistence).await;

    let thread = ThreadPersistence::create_thread(&persistence, channel.id, "Redelivery", &[])
        .await
        .unwrap();
    let message = persistence
        .create_message(&email_write(EmailMessageDraft {
            thread_id: thread.id,
            message_id: MessageId::from(format!("<redelivery-{}@example.com>", Uuid::new_v4())),
            clean_text_body: "Please answer".into(),
            ..Default::default()
        }))
        .await
        .unwrap();
    let payload = serde_json::json!({ "inbound_message": { "id": message.id } });
    let first_chain = CorrelationId::new();

    let first = persistence
        .enqueue_task(NewTask {
            targets: Vec::new(),
            source: TaskSource::Message(message.canonical_id),
            company_id: company.id,
            channel_id: channel.id,
            thread_id: None,
            task_type: "email_agent_dispatch".to_string(),
            payload: payload.clone(),
            correlation_id: first_chain,
        })
        .await
        .unwrap();

    // The same message is delivered again, and ingress resolves it to the canonical message it
    // already stored -- so the queue must hand back the task that message already has.
    let second = persistence
        .enqueue_task(NewTask {
            targets: Vec::new(),
            source: TaskSource::Message(message.canonical_id),
            company_id: company.id,
            channel_id: channel.id,
            thread_id: None,
            task_type: "email_agent_dispatch".to_string(),
            payload,
            correlation_id: CorrelationId::new(),
        })
        .await
        .unwrap();

    assert_eq!(
        second.id, first.id,
        "the duplicate returns the original task"
    );
    assert_eq!(
        second.correlation_id, first_chain,
        "and keeps the chain it already had"
    );
}

/// The census must see each kind of stall.
///
/// Asserted as "at least ours" rather than as an exact delta: the suite shares one database
/// and runs in parallel, so other tests are planting and completing tasks throughout. The
/// counting *logic* is pinned by the unit tests on [`StuckWorkCensus`]; what this checks is
/// that the SQL classifies a real row into the right bucket at all.
#[tokio::test]
async fn the_census_sees_each_kind_of_stuck_work() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let thresholds = StuckWorkThresholds::default();

    let dead = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            channel.id,
            None,
            "census-dead",
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    sqlx::query(&format!(
        "UPDATE background_tasks SET {CLEAR_TRANSITION}, status = 'dead_letter' WHERE id = $1"
    ))
    .bind(dead.id)
    .execute(&pool)
    .await
    .unwrap();

    let overdue = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            channel.id,
            None,
            "census-overdue",
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    sqlx::query(
        "UPDATE background_tasks SET run_at = CURRENT_TIMESTAMP - interval '1 hour' \
             WHERE id = $1",
    )
    .bind(overdue.id)
    .execute(&pool)
    .await
    .unwrap();

    let census = persistence.census_stuck_work(thresholds).await.unwrap();
    assert!(
        census.dead_lettered >= 1,
        "the dead-lettered task is counted"
    );
    assert!(census.queue_overdue >= 1, "the overdue task is counted");
    assert!(!census.is_quiet());

    // The discriminating case, which is safe to assert exactly because it is about one row:
    // a task queued to run now is not overdue, so flipping only `run_at` back must drop it
    // out of the bucket again.
    sqlx::query("UPDATE background_tasks SET run_at = CURRENT_TIMESTAMP WHERE id = $1")
        .bind(overdue.id)
        .execute(&pool)
        .await
        .unwrap();
    let still_overdue: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM background_tasks \
             WHERE id = $1 AND status = 'pending' \
               AND run_at < CURRENT_TIMESTAMP - $2::interval",
    )
    .bind(overdue.id)
    .bind(PgInterval::try_from(thresholds.queue_overdue_after()).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(still_overdue, 0, "a task due now is not stuck");
}

struct DelegationFixture {
    company: crate::entities::company::Company,
    task: BackgroundTask,
    outreach_id: Uuid,
    target_ids: Vec<Uuid>,
    delivery_ids: Vec<Uuid>,
    target_emails: Vec<String>,
    manager: PrincipalId,
}

async fn delegation_fixture(
    persistence: &PostgresPersistence,
    target_count: usize,
    expires_at: DateTime<Utc>,
) -> DelegationFixture {
    let (company, channel) = seed_company_and_channel(persistence).await;
    let task = enqueue_chain(persistence, company.id, channel.id, "delegation-control").await;
    let lease = claim(persistence, task.id).await;
    let thread_id = task.thread_id.unwrap();
    let suffix = Uuid::new_v4().simple().to_string();
    let mut targets = Vec::with_capacity(target_count);
    let mut delivery_ids = Vec::with_capacity(target_count);
    let mut target_emails = Vec::with_capacity(target_count);
    for index in 0..target_count {
        let email = format!("delegate-{index}-{suffix}@partner.test");
        let key = format!("delegation-{suffix}-{index}");
        let queued = delivery_fixture(
            persistence,
            DeliveryFixtureRequest {
                task_id: Some(task.id),
                recipient: &email,
                purpose: DeliveryPurpose::Outreach,
                ..DeliveryFixtureRequest::new(company.id, channel.id, thread_id, &key)
            },
        )
        .await;
        delivery_ids.push(queued.delivery.id.as_uuid());
        target_emails.push(email.clone());
        targets.push(OutreachTargetRequest {
            target: OutreachTargetIdentity::External {
                identity: qualified_email_identity(&email).unwrap(),
            },
            request: email_write(EmailMessageDraft {
                id: Uuid::new_v4(),
                thread_id,
                message_id: format!("<{key}@mailagents.test>").into(),
                sender: "support@acme.mailagents.test".into(),
                recipients_to: vec![email.into()],
                subject: "Delegated question".into(),
                clean_text_body: "Please answer".into(),
                direction: MessageDirection::Outbound,
                role: MessageRole::Agent,
                ..EmailMessageDraft::default()
            }),
            delivery: queued.delivery,
        });
    }
    let outreach_id = Uuid::new_v4();
    persistence
        .create_outreach_and_pause(CreateOutreachRequest {
            invocation: None,
            id: outreach_id,
            lease,
            company_id: company.id,
            channel_id: channel.id,
            correlation_id: task.correlation_id,
            outreach_key: format!("delegation-{suffix}"),
            required_threshold_percent: 100.0,
            expires_at,
            subject: "Delegated question".into(),
            body: "Please answer".into(),
            targets,
        })
        .await
        .unwrap();
    let target_ids = sqlx::query_scalar(
        "SELECT id FROM task_outreach_targets WHERE outreach_id = $1 ORDER BY email",
    )
    .bind(outreach_id)
    .fetch_all(&persistence.pool)
    .await
    .unwrap();
    let manager = PrincipalId::new(
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id = $1 AND user_id = $2")
            .bind(company.id)
            .bind(company.user_id)
            .fetch_one(&persistence.pool)
            .await
            .unwrap(),
    );
    DelegationFixture {
        company,
        task,
        outreach_id,
        target_ids,
        delivery_ids,
        target_emails,
        manager,
    }
}

fn delegation_command(
    fixture: &DelegationFixture,
    command_id: Uuid,
    expected_version: u64,
    reason: DelegationReason,
    operation: DelegationOperation,
) -> DelegationCommandRequest {
    DelegationCommandRequest {
        command: DelegationCommand {
            company_id: fixture.company.id,
            task_id: fixture.task.id,
            command_id,
            expected_version,
            actor: DelegationActor {
                principal_id: fixture.manager,
                authority: DelegationAuthority::CompanyManager,
            },
            reason,
            reason_detail: Some("database concurrency test".into()),
            operation,
        },
        replacement: None,
    }
}

#[tokio::test]
async fn delegation_commands_are_idempotent_fenced_and_audited() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let fixture =
        delegation_fixture(&persistence, 1, Utc::now() + chrono::Duration::hours(96)).await;
    let command_id = Uuid::new_v4();
    let request = delegation_command(
        &fixture,
        command_id,
        1,
        DelegationReason::TargetUnavailable,
        DelegationOperation::CancelTarget {
            outreach_id: fixture.outreach_id,
            target_id: fixture.target_ids[0],
        },
    );
    let (first, retry) = tokio::join!(
        persistence.execute_delegation_command(request.clone()),
        persistence.execute_delegation_command(request.clone()),
    );
    let first = first.unwrap();
    assert_eq!(retry.unwrap(), first, "the UUID returns its stored result");
    assert_eq!(first.outreach_version, 2);
    assert_eq!(
        first.delivery_cancellation,
        DeliveryCancellation::UnsentCancelled
    );

    let mut changed = request.clone();
    changed.command.reason = DelegationReason::IncorrectTarget;
    assert!(matches!(
        persistence.execute_delegation_command(changed).await,
        Err(AppError::Conflict(_))
    ));
    let stale = delegation_command(
        &fixture,
        Uuid::new_v4(),
        1,
        DelegationReason::DeadlineChanged,
        DelegationOperation::ExtendOutreach {
            outreach_id: fixture.outreach_id,
            expires_at: Utc::now() + chrono::Duration::hours(24),
        },
    );
    assert!(matches!(
        persistence.execute_delegation_command(stale).await,
        Err(AppError::Conflict(_))
    ));
    let audit: (String, String, String, i64, i64) = sqlx::query_as(
        r#"SELECT actor_kind, authority, reason, from_version, to_version
           FROM delegation_control_commands WHERE task_id = $1 AND command_id = $2"#,
    )
    .bind(fixture.task.id)
    .bind(command_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        audit,
        (
            "person".into(),
            "company_manager".into(),
            "target_unavailable".into(),
            1,
            2
        )
    );
    CompanyPersistence::delete(&persistence, fixture.company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn owning_agent_authority_is_limited_to_its_internal_delegation_scope() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let fixture =
        delegation_fixture(&persistence, 1, Utc::now() + chrono::Duration::hours(96)).await;
    let creator = PrincipalId::new(
        sqlx::query_scalar("SELECT created_by_principal_id FROM task_outreaches WHERE id = $1")
            .bind(fixture.outreach_id)
            .fetch_one(&pool)
            .await
            .unwrap(),
    );
    let mut extend = delegation_command(
        &fixture,
        Uuid::new_v4(),
        1,
        DelegationReason::DeadlineChanged,
        DelegationOperation::ExtendOutreach {
            outreach_id: fixture.outreach_id,
            expires_at: Utc::now() + chrono::Duration::hours(48),
        },
    );
    extend.command.actor = DelegationActor {
        principal_id: creator,
        authority: DelegationAuthority::OwningAgent,
    };
    assert_eq!(
        persistence
            .execute_delegation_command(extend)
            .await
            .unwrap()
            .outreach_version,
        2
    );
    for operation in [
        DelegationOperation::CancelTarget {
            outreach_id: fixture.outreach_id,
            target_id: fixture.target_ids[0],
        },
        DelegationOperation::ProceedWithPartial {
            outreach_id: fixture.outreach_id,
        },
        DelegationOperation::CancelOutreach {
            outreach_id: fixture.outreach_id,
        },
        DelegationOperation::StopTask {
            outreach_id: fixture.outreach_id,
        },
    ] {
        let mut denied = delegation_command(
            &fixture,
            Uuid::new_v4(),
            2,
            DelegationReason::NoLongerNeeded,
            operation,
        );
        denied.command.actor = DelegationActor {
            principal_id: creator,
            authority: DelegationAuthority::OwningAgent,
        };
        assert!(matches!(
            persistence.execute_delegation_command(denied).await,
            Err(AppError::NotFound(_))
        ));
    }
    CompanyPersistence::delete(&persistence, fixture.company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn cancel_target_serializes_with_delivery_claim() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let fixture =
        delegation_fixture(&persistence, 1, Utc::now() + chrono::Duration::hours(96)).await;
    let request = delegation_command(
        &fixture,
        Uuid::new_v4(),
        1,
        DelegationReason::NoLongerNeeded,
        DelegationOperation::CancelTarget {
            outreach_id: fixture.outreach_id,
            target_id: fixture.target_ids[0],
        },
    );
    let delivery_id = fixture.delivery_ids[0];
    let claim = async {
        sqlx::query_scalar::<_, Uuid>(
            r#"UPDATE message_deliveries
               SET status = 'sending', execution_id = $2, owner_worker_id = $3,
                   locked_at = CURRENT_TIMESTAMP,
                   lock_expires_at = CURRENT_TIMESTAMP + interval '5 minutes',
                   updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND status IN ('pending', 'retryable') RETURNING id"#,
        )
        .bind(delivery_id)
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .fetch_optional(&pool)
        .await
        .unwrap()
    };
    let (cancelled, claimed) = tokio::join!(persistence.execute_delegation_command(request), claim);
    let cancelled = cancelled.unwrap();
    assert_eq!(
        cancelled.delivery_cancellation,
        if claimed.is_some() {
            DeliveryCancellation::MayHaveBeenReceived
        } else {
            DeliveryCancellation::UnsentCancelled
        }
    );
    let status: String = sqlx::query_scalar("SELECT status FROM message_deliveries WHERE id = $1")
        .bind(delivery_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        status,
        if claimed.is_some() {
            "sending"
        } else {
            "dead_letter"
        }
    );
    CompanyPersistence::delete(&persistence, fixture.company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn proceed_partial_serializes_its_exact_snapshot_with_a_reply() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let fixture =
        delegation_fixture(&persistence, 2, Utc::now() + chrono::Duration::hours(96)).await;
    let response = persistence
        .create_message(&email_write(EmailMessageDraft {
            id: Uuid::new_v4(),
            thread_id: fixture.task.thread_id.unwrap(),
            message_id: format!("<racing-reply-{}@partner.test>", Uuid::new_v4()).into(),
            sender: fixture.target_emails[0].clone().into(),
            subject: "Re: Delegated question".into(),
            clean_text_body: "Available result".into(),
            direction: MessageDirection::Inbound,
            role: MessageRole::Human,
            ..EmailMessageDraft::default()
        }))
        .await
        .unwrap();
    let matched = crate::entities::outreach::OutreachReplyMatch {
        outreach_id: fixture.outreach_id,
        task_id: fixture.task.id,
        target_id: fixture.target_ids[0],
        target_email: fixture.target_emails[0].clone().into(),
    };
    let request = delegation_command(
        &fixture,
        Uuid::new_v4(),
        1,
        DelegationReason::PartialResultsAccepted,
        DelegationOperation::ProceedWithPartial {
            outreach_id: fixture.outreach_id,
        },
    );
    let (decision, reply) = tokio::join!(
        persistence.execute_delegation_command(request),
        persistence.record_outreach_reply(&matched, response.id),
    );
    let decision = decision.unwrap();
    reply.unwrap();
    let disposition: String = sqlx::query_scalar(
        "SELECT disposition FROM task_outreach_replies WHERE response_association_id = $1",
    )
    .bind(response.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    match disposition.as_str() {
        "counted" => assert_eq!(decision.response_association_ids, vec![response.id]),
        "late" => assert!(decision.response_association_ids.is_empty()),
        other => panic!("unexpected racing reply disposition {other}"),
    }
    let status: String = sqlx::query_scalar("SELECT status FROM task_outreaches WHERE id = $1")
        .bind(fixture.outreach_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "proceed_partial");
    CompanyPersistence::delete(&persistence, fixture.company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn extending_an_expired_outreach_serializes_with_the_timeout_sweep() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let fixture =
        delegation_fixture(&persistence, 1, Utc::now() + chrono::Duration::hours(1)).await;
    sqlx::query(
        r#"UPDATE task_outreaches
           SET created_at = CURRENT_TIMESTAMP - interval '2 hours',
               expires_at = CURRENT_TIMESTAMP - interval '1 second'
           WHERE id = $1"#,
    )
    .bind(fixture.outreach_id)
    .execute(&pool)
    .await
    .unwrap();
    let new_expiry = Utc::now() + chrono::Duration::hours(48);
    let request = delegation_command(
        &fixture,
        Uuid::new_v4(),
        1,
        DelegationReason::DeadlineChanged,
        DelegationOperation::ExtendOutreach {
            outreach_id: fixture.outreach_id,
            expires_at: new_expiry,
        },
    );
    let (extended, swept) = tokio::join!(
        persistence.execute_delegation_command(request),
        persistence.mark_outreach_timeout_pending(fixture.outreach_id),
    );
    extended.unwrap();
    swept.unwrap();
    let (status, expires_at): (String, DateTime<Utc>) =
        sqlx::query_as("SELECT status, expires_at FROM task_outreaches WHERE id = $1")
            .bind(fixture.outreach_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "waiting");
    assert_eq!(expires_at, new_expiry);
    assert_eq!(
        persistence
            .get_task_by_id(fixture.task.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::WaitingForThirdPartyReply
    );
    CompanyPersistence::delete(&persistence, fixture.company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn internal_cancel_serializes_with_child_completion_and_revokes_old_execution() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let fixture =
        delegation_fixture(&persistence, 1, Utc::now() + chrono::Duration::hours(96)).await;
    let target_id = fixture.target_ids[0];
    sqlx::query(
        r#"UPDATE task_outreach_targets
           SET target_kind = 'internal_channel', internal_channel_id = $2,
               external_transport = NULL, external_namespace = NULL, external_subject = NULL
           WHERE id = $1"#,
    )
    .bind(target_id)
    .bind(fixture.task.channel_id)
    .execute(&pool)
    .await
    .unwrap();

    let provider_key = format!("<internal-{}@mailagents.test>", Uuid::new_v4());
    sqlx::query(
        r#"UPDATE message_delivery_parts
           SET status = 'delivered', provider_message_key = $2,
               request_started_at = CURRENT_TIMESTAMP, delivered_at = CURRENT_TIMESTAMP
           WHERE delivery_id = $1"#,
    )
    .bind(fixture.delivery_ids[0])
    .bind(&provider_key)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        r#"UPDATE message_deliveries SET status = 'delivered', delivered_at = CURRENT_TIMESTAMP
           WHERE id = $1"#,
    )
    .bind(fixture.delivery_ids[0])
    .execute(&pool)
    .await
    .unwrap();
    let child_message = persistence
        .create_message(&email_write(EmailMessageDraft {
            id: Uuid::new_v4(),
            thread_id: fixture.task.thread_id.unwrap(),
            message_id: format!("<child-{}@mailagents.test>", Uuid::new_v4()).into(),
            sender: "internal@mailagents.test".into(),
            subject: "Internal request".into(),
            clean_text_body: "Work on this".into(),
            direction: MessageDirection::Inbound,
            role: MessageRole::Human,
            ..EmailMessageDraft::default()
        }))
        .await
        .unwrap();
    let binding_id: Uuid =
        sqlx::query_scalar("SELECT destination_binding_id FROM message_deliveries WHERE id = $1")
            .bind(fixture.delivery_ids[0])
            .fetch_one(&pool)
            .await
            .unwrap();
    sqlx::query(
        r#"INSERT INTO external_messages
               (id, company_id, binding_id, external_message_key, message_id)
           VALUES ($1, $2, $3, $4, $5)"#,
    )
    .bind(Uuid::new_v4())
    .bind(fixture.company.id)
    .bind(binding_id)
    .bind(&provider_key)
    .bind(child_message.canonical_id.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    let child = persistence
        .enqueue_task(NewTask {
            targets: Vec::new(),
            source: TaskSource::Message(child_message.canonical_id),
            company_id: fixture.company.id,
            channel_id: fixture.task.channel_id,
            thread_id: Some(fixture.task.thread_id.unwrap()),
            task_type: "internal-child".into(),
            payload: serde_json::json!({}),
            correlation_id: fixture.task.correlation_id,
        })
        .await
        .unwrap();
    let child_lease = claim(&persistence, child.id).await;
    let request = delegation_command(
        &fixture,
        Uuid::new_v4(),
        1,
        DelegationReason::NoLongerNeeded,
        DelegationOperation::CancelTarget {
            outreach_id: fixture.outreach_id,
            target_id,
        },
    );
    let (cancelled, completed) = tokio::join!(
        persistence.execute_delegation_command(request),
        persistence.mark_task_completed(child_lease),
    );
    match (cancelled, completed.unwrap()) {
        (Ok(_), false) => {
            assert_eq!(
                persistence
                    .get_task_by_id(child.id)
                    .await
                    .unwrap()
                    .unwrap()
                    .status,
                TaskStatus::Stopped
            );
            assert!(
                !persistence
                    .renew_task_lease(child_lease, Utc::now() + chrono::Duration::minutes(5))
                    .await
                    .unwrap(),
                "the cancelled child's old execution fence is revoked"
            );
        }
        (Err(AppError::Conflict(_)), true) => assert_eq!(
            persistence
                .get_task_by_id(child.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            TaskStatus::Completed
        ),
        other => panic!("race must have exactly one winner, got {other:?}"),
    }
    CompanyPersistence::delete(&persistence, fixture.company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn internal_reassignment_serializes_with_reply_and_preserves_old_target_history() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let fixture =
        delegation_fixture(&persistence, 1, Utc::now() + chrono::Duration::hours(96)).await;
    let target_id = fixture.target_ids[0];
    sqlx::query(
        r#"UPDATE task_outreach_targets
           SET target_kind = 'internal_channel', internal_channel_id = $2,
               external_transport = NULL, external_namespace = NULL, external_subject = NULL
           WHERE id = $1"#,
    )
    .bind(target_id)
    .bind(fixture.task.channel_id)
    .execute(&pool)
    .await
    .unwrap();
    let agent_id = seed_channel_agent(&persistence, fixture.company.id, "replacement").await;
    let replacement_channel = ChannelPersistence::create(
        &persistence,
        fixture.company.id,
        ChannelWrite {
            name: "Replacement".into(),
            slug: format!("replacement-{}", Uuid::new_v4().simple()),
            agent_ids: Some(vec![agent_id]),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let key = format!("replacement-{}", Uuid::new_v4().simple());
    let mut queued = delivery_fixture(
        &persistence,
        DeliveryFixtureRequest {
            task_id: Some(fixture.task.id),
            recipient: "replacement@internal.test",
            purpose: DeliveryPurpose::Outreach,
            ..DeliveryFixtureRequest::new(
                fixture.company.id,
                fixture.task.channel_id,
                fixture.task.thread_id.unwrap(),
                &key,
            )
        },
    )
    .await;
    let replacement_request = email_write(EmailMessageDraft {
        id: Uuid::new_v4(),
        thread_id: fixture.task.thread_id.unwrap(),
        message_id: format!("<{key}@mailagents.test>").into(),
        sender: "support@acme.mailagents.test".into(),
        recipients_to: vec!["replacement@internal.test".into()],
        subject: "Reassigned question".into(),
        clean_text_body: "Please answer".into(),
        direction: MessageDirection::Outbound,
        role: MessageRole::Agent,
        ..EmailMessageDraft::default()
    });
    queued.delivery.message_id = replacement_request.id;
    let replacement = OutreachTargetRequest {
        target: OutreachTargetIdentity::InternalChannel {
            channel_id: replacement_channel.id,
        },
        request: replacement_request,
        delivery: queued.delivery,
    };
    let response = persistence
        .create_message(&email_write(EmailMessageDraft {
            id: Uuid::new_v4(),
            thread_id: fixture.task.thread_id.unwrap(),
            message_id: format!("<reassign-race-{}@internal.test>", Uuid::new_v4()).into(),
            sender: fixture.target_emails[0].clone().into(),
            subject: "Re: Delegated question".into(),
            clean_text_body: "Old target result".into(),
            direction: MessageDirection::Inbound,
            role: MessageRole::Human,
            ..EmailMessageDraft::default()
        }))
        .await
        .unwrap();
    let matched = crate::entities::outreach::OutreachReplyMatch {
        outreach_id: fixture.outreach_id,
        task_id: fixture.task.id,
        target_id,
        target_email: fixture.target_emails[0].clone().into(),
    };
    let mut request = delegation_command(
        &fixture,
        Uuid::new_v4(),
        1,
        DelegationReason::IncorrectTarget,
        DelegationOperation::ReassignInternalTarget {
            outreach_id: fixture.outreach_id,
            target_id,
            new_channel_id: replacement_channel.id,
        },
    );
    request.replacement = Some(replacement);
    let (reassigned, replied) = tokio::join!(
        persistence.execute_delegation_command(request),
        persistence.record_outreach_reply(&matched, response.id),
    );
    replied.unwrap();
    let old_status: String =
        sqlx::query_scalar("SELECT status FROM task_outreach_targets WHERE id = $1")
            .bind(target_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    match reassigned {
        Ok(result) => {
            assert_eq!(old_status, "superseded");
            let replacement_id = result.replacement_target_id.unwrap();
            let link: (String, Uuid) = sqlx::query_as(
                "SELECT status, replaces_target_id FROM task_outreach_targets WHERE id = $1",
            )
            .bind(replacement_id)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(link, ("active".into(), target_id));
            let disposition: String = sqlx::query_scalar(
                "SELECT disposition FROM task_outreach_replies WHERE response_association_id = $1",
            )
            .bind(response.id)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(disposition, "late");
        }
        Err(AppError::Conflict(_)) => assert_eq!(old_status, "responded"),
        other => panic!("unexpected reassignment race result: {other:?}"),
    }
    CompanyPersistence::delete(&persistence, fixture.company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn stop_task_serializes_with_final_dispatch_without_duplicate_customer_reply() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let fixture =
        delegation_fixture(&persistence, 1, Utc::now() + chrono::Duration::hours(96)).await;
    persistence
        .execute_delegation_command(delegation_command(
            &fixture,
            Uuid::new_v4(),
            1,
            DelegationReason::PartialResultsAccepted,
            DelegationOperation::ProceedWithPartial {
                outreach_id: fixture.outreach_id,
            },
        ))
        .await
        .unwrap();
    let lease = claim(&persistence, fixture.task.id).await;
    let reply = AgentReply {
        message: MessageWrite::internal(
            fixture.task.thread_id.unwrap(),
            MessageAuthorWrite::Platform,
            "Re: Final",
            "Final answer",
            MessageDirection::Outbound,
            MessageRole::Agent,
            fixture.task.correlation_id,
        )
        .external_conversation(),
        also_in_threads: Vec::new(),
    };
    let side_thread = persistence
        .create_thread(fixture.task.channel_id, "Stop race delivery", &[])
        .await
        .unwrap();
    let mut delivery = delivery_fixture(
        &persistence,
        DeliveryFixtureRequest {
            task_id: Some(fixture.task.id),
            source_key: "stop-final-race",
            ..DeliveryFixtureRequest::new(
                fixture.company.id,
                fixture.task.channel_id,
                side_thread.id,
                "stop-final-race",
            )
        },
    )
    .await
    .delivery;
    delivery.message_id = reply.message.id;
    let stop = delegation_command(
        &fixture,
        Uuid::new_v4(),
        2,
        DelegationReason::TaskStopped,
        DelegationOperation::StopTask {
            outreach_id: fixture.outreach_id,
        },
    );
    let (stopped, dispatched) = tokio::join!(
        persistence.execute_delegation_command(stop),
        persistence.commit_agent_dispatch(AgentDispatchCommit {
            lease,
            reply: &reply,
            deliveries: vec![delivery],
            review_candidate: None,
            payload: serde_json::json!({"final": true}),
            complete_outreach: true,
        }),
    );
    match (stopped, dispatched.unwrap()) {
        (Ok(_), DispatchCommit::LeaseLost) => assert_eq!(
            persistence
                .get_task_by_id(fixture.task.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            TaskStatus::Stopped
        ),
        (Err(AppError::Conflict(_)), DispatchCommit::Committed { .. }) => assert_eq!(
            persistence
                .get_task_by_id(fixture.task.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            TaskStatus::Completed
        ),
        other => panic!("stop/final-dispatch race must have one winner, got {other:?}"),
    }
    let replies: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM message_deliveries
           WHERE task_id = $1 AND idempotency_key LIKE '%stop-final-race%'"#,
    )
    .bind(fixture.task.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        replies <= 1,
        "the recovery path cannot queue a second customer reply"
    );
    CompanyPersistence::delete(&persistence, fixture.company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn database_enforces_complete_outreach_and_target_transition_matrices() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let fixture =
        delegation_fixture(&persistence, 1, Utc::now() + chrono::Duration::hours(96)).await;
    let outreach_states = [
        "waiting",
        "threshold_met",
        "timeout_pending_approval",
        "proceed_partial",
        "cancelled",
        "completed",
    ];
    let outreach_allowed = |from: &str, to: &str| {
        from == to
            || matches!(
                (from, to),
                (
                    "waiting",
                    "threshold_met" | "timeout_pending_approval" | "proceed_partial" | "cancelled"
                ) | (
                    "timeout_pending_approval",
                    "waiting" | "threshold_met" | "proceed_partial" | "cancelled"
                ) | (
                    "threshold_met" | "proceed_partial",
                    "completed" | "cancelled"
                )
            )
    };
    for from in outreach_states {
        for to in outreach_states {
            let id = Uuid::new_v4();
            sqlx::query(
                r#"INSERT INTO task_outreaches
                       (id, task_id, company_id, status, required_threshold_percent, expires_at,
                        outreach_key, subject, body)
                   VALUES ($1, $2, $3, $4, 100, CURRENT_TIMESTAMP + interval '1 day',
                           $5, 'matrix', 'matrix')"#,
            )
            .bind(id)
            .bind(fixture.task.id)
            .bind(fixture.company.id)
            .bind(from)
            .bind(format!("matrix-{id}"))
            .execute(&pool)
            .await
            .unwrap();
            let changed = sqlx::query("UPDATE task_outreaches SET status = $2 WHERE id = $1")
                .bind(id)
                .bind(to)
                .execute(&pool)
                .await;
            assert_eq!(
                changed.is_ok(),
                outreach_allowed(from, to),
                "{from} -> {to}"
            );
        }
    }

    let response = persistence
        .create_message(&email_write(EmailMessageDraft {
            id: Uuid::new_v4(),
            thread_id: fixture.task.thread_id.unwrap(),
            message_id: format!("<matrix-{}@partner.test>", Uuid::new_v4()).into(),
            sender: "matrix@partner.test".into(),
            subject: "Matrix".into(),
            clean_text_body: "Matrix".into(),
            direction: MessageDirection::Inbound,
            role: MessageRole::Human,
            ..EmailMessageDraft::default()
        }))
        .await
        .unwrap();
    let target_states = ["active", "responded", "cancelled", "superseded", "expired"];
    for from in target_states {
        for to in target_states {
            let id = Uuid::new_v4();
            sqlx::query(
                r#"INSERT INTO task_outreach_targets
                       (id, outreach_id, company_id, email, target_kind,
                        external_transport, external_namespace, external_subject, status,
                        responded_at, response_association_id)
                   VALUES ($1, $2, $3, $4, 'external', 'email', 'email', $4, $5,
                           CASE WHEN $5 = 'responded' THEN CURRENT_TIMESTAMP END,
                           CASE WHEN $5 = 'responded' THEN $6 END)"#,
            )
            .bind(id)
            .bind(fixture.outreach_id)
            .bind(fixture.company.id)
            .bind(format!("matrix-{id}@partner.test"))
            .bind(from)
            .bind(response.id)
            .execute(&pool)
            .await
            .unwrap();
            let changed = sqlx::query(
                r#"UPDATE task_outreach_targets
                   SET status = $2,
                       responded_at = CASE WHEN $2 = 'responded' THEN CURRENT_TIMESTAMP END,
                       response_association_id = CASE WHEN $2 = 'responded' THEN $3 END
                   WHERE id = $1"#,
            )
            .bind(id)
            .bind(to)
            .bind(response.id)
            .execute(&pool)
            .await;
            assert_eq!(
                changed.is_ok(),
                from == to || from == "active",
                "{from} -> {to}"
            );
        }
    }
    CompanyPersistence::delete(&persistence, fixture.company.id)
        .await
        .unwrap();
}

async fn assert_note_request_uses_platform_author(pool: &sqlx::PgPool, company_id: Uuid) {
    let count: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM messages AS message
            JOIN principals AS author
              ON (author.company_id, author.id) = (message.company_id, message.author_principal_id)
           WHERE message.company_id = $1 AND message.subject = 'Internal note request'
             AND author.kind = 'system' AND message.authored_identity_id IS NULL"#,
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(
        count, 1,
        "retries reuse one platform-authored event without a synthetic transport identity"
    );
}
