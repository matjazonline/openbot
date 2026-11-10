use super::*;
use crate::application::local_agents::lifecycle::LocalAgentMaintenance;
use crate::application::local_agents::management::{LocalAgentManagementStore, StartSession};
use crate::application::local_agents::protocol::*;
use protocol::test_support::{Context, agent, turn};

async fn age_settlement(c: &Context, id: OperationId) {
    let mut tx = c.f.database.pool.begin().await.unwrap();
    sqlx::query("ALTER TABLE local_agent_operations DISABLE TRIGGER local_operation_settlement")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE local_agent_operations SET settled_at=clock_timestamp()-interval '91 days' WHERE company_id=$1 AND id=$2")
        .bind(c.f.company).bind(id.as_uuid()).execute(&mut *tx).await.unwrap();
    sqlx::query("ALTER TABLE local_agent_operations ENABLE TRIGGER local_operation_settlement")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

async fn age_session(c: &Context, session: SessionId) {
    let mut tx = c.f.database.pool.begin().await.unwrap();
    sqlx::query("ALTER TABLE local_agent_sessions DISABLE TRIGGER local_session_inputs_immutable")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE local_agent_sessions SET created_at=clock_timestamp()-interval '100 days' WHERE company_id=$1 AND id=$2")
        .bind(c.f.company).bind(session.as_uuid()).execute(&mut *tx).await.unwrap();
    sqlx::query("ALTER TABLE local_agent_sessions ENABLE TRIGGER local_session_inputs_immutable")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn cloudroom_retention_unknown_native_work_survives_and_compacted_receipt_replays_under_current_fence()
 {
    let c = Context::new().await.unwrap();
    let p = c.f.persistence.as_ref();
    let operation = p.enqueue_operation(&c.admission(c.send())).await.unwrap();
    p.pending_commands(c.fence).await.unwrap();
    let receipt = agent(3, operation.id(), LocalEvidence::Receipt {});
    for observation in [
        receipt.clone(),
        agent(4, operation.id(), LocalEvidence::SubmissionIntent {}),
        turn(
            5,
            operation.id(),
            "turn_retention",
            TurnState::Running,
            None,
        ),
        agent(6, operation.id(), LocalEvidence::Uncertain {}),
    ] {
        p.accept_observation(c.fence, &observation).await.unwrap();
    }
    sqlx::query("UPDATE local_agent_observations SET accepted_at=clock_timestamp()-interval '8 days' WHERE company_id=$1")
        .bind(c.f.company).execute(&c.f.database.pool).await.unwrap();
    p.retain_local_agents(None).await.unwrap();
    let unsettled:(bool,bool)=sqlx::query_as("SELECT settled_at IS NULL,EXISTS(SELECT 1 FROM local_agent_observations WHERE company_id=$1 AND operation_id=$2 AND observation IS NOT NULL) FROM local_agent_operations WHERE company_id=$1 AND id=$2")
        .bind(c.f.company).bind(operation.id().as_uuid()).fetch_one(&c.f.database.pool).await.unwrap();
    assert_eq!(
        unsettled,
        (true, true),
        "uncertain evidence must survive detailed compaction"
    );
    p.accept_observation(
        c.fence,
        &turn(
            7,
            operation.id(),
            "turn_retention",
            TurnState::Completed,
            Some("kept native summary"),
        ),
    )
    .await
    .unwrap();
    let page = p.retain_local_agents(None).await.unwrap();
    assert_eq!(page.compacted_observations, 4);
    let value:Value=sqlx::query_scalar("SELECT jsonb_build_object('output',output,'settled',settled_at IS NOT NULL,'details',(SELECT count(*) FROM local_agent_observations WHERE company_id=$1 AND operation_id=$2 AND observation IS NOT NULL)) FROM local_agent_operations WHERE company_id=$1 AND id=$2")
        .bind(c.f.company).bind(operation.id().as_uuid()).fetch_one(&c.f.database.pool).await.unwrap();
    assert_eq!(
        value,
        json!({"output":"kept native summary","settled":true,"details":1})
    );
    assert_eq!(p.accept_observation(c.fence, &receipt).await.unwrap(), 7);
    assert!(
        p.accept_observation(
            c.fence,
            &agent(3, operation.id(), LocalEvidence::Uncertain {})
        )
        .await
        .is_err()
    );
    let (current, _) = c.reconnect(c.journal, 7).await;
    assert!(matches!(
        p.accept_observation(c.fence, &receipt).await,
        Err(AppError::InvalidCredentials)
    ));
    assert_eq!(p.accept_observation(current, &receipt).await.unwrap(), 7);
    age_settlement(&c, operation.id()).await;
    age_session(&c, c.session.id).await;
    let page = p.retain_local_agents(None).await.unwrap();
    assert_eq!((page.removed_operations, page.removed_sessions), (1, 1));
    let runtime = p
        .machine_runtime(c.f.owner, c.f.company, c.paired.receipt.machine.id)
        .await
        .unwrap();
    assert!(
        runtime.last_native_success_at.is_some(),
        "native success survives summary retention"
    );
    assert_eq!(runtime.unsettled_effects, 0);
}

#[tokio::test]
async fn cloudroom_retention_competing_collectors_remove_only_bounded_old_summaries_and_keep_unknown_work()
 {
    let c = Context::new().await.unwrap();
    let p = c.f.persistence.as_ref();
    let unknown = p.enqueue_operation(&c.admission(c.send())).await.unwrap();
    let control = c.admission(LocalCommand::Inspect {
        session_id: c.session.id,
        thread_id: CloudroomThreadId::try_from("thr_frozen".to_owned()).unwrap(),
    });
    sqlx::query("INSERT INTO local_agent_operations(company_id,id,machine_id,installation_id,session_id,logical_key,actor_id,command,payload_digest,deadline,outcome)
        SELECT $1,gen_random_uuid(),$2,$3,$4,'old-control:'||ordinal,$5,$6,$7,$8,'failed' FROM generate_series(1,130) AS old(ordinal)")
        .bind(c.f.company).bind(c.paired.receipt.machine.id.as_uuid()).bind(c.paired.receipt.machine.installation_id.as_uuid())
        .bind(c.session.id.as_uuid()).bind(c.f.owner).bind(encode(control.operation.command()).unwrap())
        .bind(control.operation.payload_digest().as_str()).bind(control.operation.deadline()).execute(&c.f.database.pool).await.unwrap();
    let mut tx = c.f.database.pool.begin().await.unwrap();
    sqlx::query("ALTER TABLE local_agent_operations DISABLE TRIGGER local_operation_settlement")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE local_agent_operations SET settled_at=clock_timestamp()-interval '91 days' WHERE company_id=$1 AND logical_key LIKE 'old-control:%'")
        .bind(c.f.company).execute(&mut *tx).await.unwrap();
    sqlx::query("ALTER TABLE local_agent_operations ENABLE TRIGGER local_operation_settlement")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let (one, two) = tokio::join!(p.retain_local_agents(None), p.retain_local_agents(None));
    let (one, two) = (one.unwrap(), two.unwrap());
    assert!(one.removed_operations <= 128 && two.removed_operations <= 128);
    let removed = one.removed_operations + two.removed_operations;
    assert!((128..=130).contains(&removed));
    let remaining:i64=sqlx::query_scalar("SELECT count(*) FROM local_agent_operations WHERE company_id=$1 AND logical_key LIKE 'old-control:%'")
        .bind(c.f.company).fetch_one(&c.f.database.pool).await.unwrap();
    assert_eq!(remaining, 130 - removed as i64);
    p.retain_local_agents(None).await.unwrap();
    let kept:i64=sqlx::query_scalar("SELECT count(*) FROM local_agent_operations WHERE company_id=$1 AND id=$2 AND settled_at IS NULL")
        .bind(c.f.company).bind(unknown.id().as_uuid()).fetch_one(&c.f.database.pool).await.unwrap();
    assert_eq!(kept, 1);
}

#[tokio::test]
async fn cloudroom_retention_new_manual_requests_require_current_timestamp_and_replay_current_identity()
 {
    let c = Context::new().await.unwrap();
    let p = c.f.persistence.as_ref();
    let conversation: Uuid = sqlx::query_scalar(
        "SELECT conversation_id FROM local_agent_sessions WHERE company_id=$1 AND id=$2",
    )
    .bind(c.f.company)
    .bind(c.session.id.as_uuid())
    .fetch_one(&c.f.database.pool)
    .await
    .unwrap();
    let mut request = StartSession {
        actor: c.f.owner,
        company: c.f.company,
        profile_id: c.session.profile.id,
        conversation_id: conversation,
        request_id: OperationId::new(Uuid::new_v4()).unwrap(),
        prompt: LocalPrompt::try_from("explicit manual action".to_owned()).unwrap(),
    };
    assert!(matches!(
        p.start_session(&request).await,
        Err(AppError::BadRequest(_))
    ));
    for issued in [
        Utc::now() - chrono::Duration::days(91),
        Utc::now() + chrono::Duration::minutes(3),
    ] {
        let timestamp = uuid::Timestamp::from_unix(
            uuid::NoContext,
            issued.timestamp() as u64,
            issued.timestamp_subsec_nanos(),
        );
        request.request_id = OperationId::new(Uuid::new_v7(timestamp)).unwrap();
        assert!(matches!(
            p.start_session(&request).await,
            Err(AppError::Conflict(_))
        ));
    }
    let current = StartSession {
        request_id: OperationId::new(Uuid::now_v7()).unwrap(),
        ..request
    };
    let saved = p.start_session(&current).await.unwrap();
    assert_eq!(saved.operation.id(), current.request_id);
    assert_eq!(
        p.start_session(&current)
            .await
            .unwrap()
            .operation
            .payload_digest(),
        saved.operation.payload_digest()
    );
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM local_agent_operations WHERE company_id=$1 AND command->>'kind'='create'")
        .bind(c.f.company).fetch_one(&c.f.database.pool).await.unwrap();
    assert_eq!(count, 1, "invalid old/future/v4 IDs grant no admission");
}

#[tokio::test]
async fn cloudroom_retention_saved_old_manual_replay_survives_age_then_collected_id_cannot_create_again()
 {
    let c = Context::new().await.unwrap();
    let p = c.f.persistence.as_ref();
    let issued = Utc::now() - chrono::Duration::days(100);
    let timestamp = uuid::Timestamp::from_unix(
        uuid::NoContext,
        issued.timestamp() as u64,
        issued.timestamp_subsec_nanos(),
    );
    let id = OperationId::new(Uuid::new_v7(timestamp)).unwrap();
    let session = SessionId::new(id.as_uuid()).unwrap();
    // A historically admitted request fixture keeps its original encoded ID; no
    // production clock or freshness override is introduced for age compression.
    sqlx::query("INSERT INTO local_agent_sessions(company_id,id,machine_id,installation_id,profile_id,selection,conversation_id)
        SELECT company_id,$3,machine_id,installation_id,profile_id,selection,conversation_id
        FROM local_agent_sessions WHERE company_id=$1 AND id=$2")
        .bind(c.f.company).bind(c.session.id.as_uuid()).bind(session.as_uuid()).execute(&c.f.database.pool).await.unwrap();
    let prompt = LocalPrompt::try_from("old explicitly authorized request".to_owned()).unwrap();
    let operation = LocalAgentOperation::queued(
        id,
        LocalCommand::Create {
            session_id: session,
            selection: c.session.profile.selection.clone(),
            prompt: prompt.clone(),
        },
        Utc::now() + chrono::Duration::minutes(30),
    )
    .unwrap();
    p.enqueue_operation(&OperationAdmission {
        company_id: c.f.company,
        actor_id: c.f.owner,
        machine_id: c.paired.receipt.machine.id,
        logical_key: LogicalInvocationKey::parse(format!("direct:{id}")).unwrap(),
        operation: operation.clone(),
    })
    .await
    .unwrap();
    p.pending_commands(c.fence).await.unwrap();
    p.accept_observation(c.fence, &agent(3, id, LocalEvidence::Receipt {}))
        .await
        .unwrap();
    p.accept_observation(
        c.fence,
        &agent(
            4,
            id,
            LocalEvidence::Refused {
                reason: LocalRefusal::ProviderUnavailable,
            },
        ),
    )
    .await
    .unwrap();
    let conversation = sqlx::query_scalar(
        "SELECT conversation_id FROM local_agent_sessions WHERE company_id=$1 AND id=$2",
    )
    .bind(c.f.company)
    .bind(session.as_uuid())
    .fetch_one(&c.f.database.pool)
    .await
    .unwrap();
    let request = StartSession {
        actor: c.f.owner,
        company: c.f.company,
        profile_id: c.session.profile.id,
        conversation_id: conversation,
        request_id: id,
        prompt,
    };
    assert_eq!(
        p.start_session(&request)
            .await
            .unwrap()
            .operation
            .payload_digest(),
        operation.payload_digest()
    );
    age_settlement(&c, id).await;
    age_session(&c, session).await;
    let page = p.retain_local_agents(None).await.unwrap();
    assert_eq!((page.removed_operations, page.removed_sessions), (1, 1));
    assert!(matches!(
        p.start_session(&request).await,
        Err(AppError::Conflict(_))
    ));
    let counts:(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM local_agent_operations WHERE company_id=$1 AND id=$2),(SELECT count(*) FROM local_agent_sessions WHERE company_id=$1 AND id=$2)")
        .bind(c.f.company).bind(id.as_uuid()).fetch_one(&c.f.database.pool).await.unwrap();
    assert_eq!(counts, (0, 0));
}

#[tokio::test]
async fn cloudroom_lifecycle_mapping_retirement_is_bounded_read_only_and_current_installation_fenced()
 {
    let c = Context::new().await.unwrap();
    let p = c.f.persistence.as_ref();
    let absent = SessionId::new(Uuid::new_v4()).unwrap();
    let request = MappingRetirementRequest {
        nonce: Uuid::new_v4(),
        sessions: MappingRetirementSessions::parse("mapping page", vec![c.session.id, absent])
            .unwrap(),
    };
    assert_eq!(
        p.retired_mappings(c.fence, &request)
            .await
            .unwrap()
            .as_ref(),
        &[absent]
    );
    assert!(
        p.session(c.f.company, c.session.id)
            .await
            .unwrap()
            .is_some()
    );
    let invalid = MappingRetirementRequest {
        nonce: Uuid::nil(),
        sessions: request.sessions.clone(),
    };
    assert!(matches!(
        p.retired_mappings(c.fence, &invalid).await,
        Err(AppError::BadRequest(_))
    ));
    let invalid = MappingRetirementRequest {
        nonce: Uuid::new_v4(),
        sessions: MappingRetirementSessions::parse("duplicate page", vec![absent, absent]).unwrap(),
    };
    assert!(matches!(
        p.retired_mappings(c.fence, &invalid).await,
        Err(AppError::BadRequest(_))
    ));
    assert!(MappingRetirementSessions::parse("overlong page", vec![absent; 101]).is_err());
    let mut foreign = c.fence;
    foreign.machine.company_id = c.f.foreign;
    assert!(matches!(
        p.retired_mappings(foreign, &request).await,
        Err(AppError::InvalidCredentials)
    ));
    let (current, _) = c.reconnect(c.journal, 2).await;
    assert!(matches!(
        p.retired_mappings(c.fence, &request).await,
        Err(AppError::InvalidCredentials)
    ));
    assert!(p.finish_reconciliation(current).await.unwrap());
    assert_eq!(
        p.retired_mappings(current, &request)
            .await
            .unwrap()
            .as_ref(),
        &[absent]
    );
}
#[tokio::test]
async fn cloudroom_management_durable_status_distinguishes_queue_permission_uncertainty_gaps_and_native_success()
 {
    let c = Context::new().await.unwrap();
    let p = c.f.persistence.as_ref();
    let machine = c.paired.receipt.machine.id;
    let operation = p.enqueue_operation(&c.admission(c.send())).await.unwrap();
    let queued = p
        .machine_runtime(c.f.owner, c.f.company, machine)
        .await
        .unwrap();
    assert_eq!(queued.outstanding_commands, 1);
    assert!(queued.oldest_outstanding_at.is_some());
    assert!(queued.last_native_success_at.is_none());
    p.pending_commands(c.fence).await.unwrap();
    for observation in [
        agent(3, operation.id(), LocalEvidence::Receipt {}),
        agent(4, operation.id(), LocalEvidence::SubmissionIntent {}),
        turn(
            5,
            operation.id(),
            "turn_metrics",
            TurnState::WaitingForInput,
            None,
        ),
    ] {
        p.accept_observation(c.fence, &observation).await.unwrap();
    }
    let waiting = p
        .machine_runtime(c.f.owner, c.f.company, machine)
        .await
        .unwrap();
    assert_eq!(waiting.permission_waits, 1);
    assert!(waiting.last_native_success_at.is_none());
    p.accept_observation(
        c.fence,
        &agent(6, operation.id(), LocalEvidence::Uncertain {}),
    )
    .await
    .unwrap();
    let (current, _) = c.reconnect(c.journal, 6).await;
    let recovery = p
        .machine_runtime(c.f.owner, c.f.company, machine)
        .await
        .unwrap();
    assert_eq!(recovery.uncertain_outcomes, 1);
    assert_eq!(recovery.reconciliation_gaps, 1);
    assert!(!recovery.reconciled);
    assert!(recovery.last_native_success_at.is_none());
    p.accept_observation(
        current,
        &turn(
            7,
            operation.id(),
            "turn_metrics",
            TurnState::Completed,
            Some("private output sentinel"),
        ),
    )
    .await
    .unwrap();
    let completed = p
        .machine_runtime(c.f.owner, c.f.company, machine)
        .await
        .unwrap();
    assert_eq!(completed.unsettled_effects, 0);
    assert_eq!(completed.permission_waits, 0);
    assert_eq!(completed.reconciliation_gaps, 0);
    assert!(completed.last_native_success_at.is_some());
    assert!(
        !serde_json::to_string(&completed)
            .unwrap()
            .contains("private output sentinel")
    );
    for _ in 0..MAX_UNSETTLED_EFFECTS {
        p.enqueue_operation(&c.admission(c.send())).await.unwrap();
    }
    assert!(
        p.machine_runtime(c.f.owner, c.f.company, machine)
            .await
            .unwrap()
            .backpressured
    );
}
