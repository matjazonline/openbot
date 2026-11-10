use super::{test_support::*, *};

#[tokio::test]
async fn cloudroom_commands_uncertain_native_work_keeps_capacity_until_known_terminal_evidence() {
    let Some(c) = Context::new().await else {
        return;
    };
    let mut operations = Vec::new();
    for _ in 0..8 {
        operations.push(
            c.f.persistence
                .enqueue_operation(&c.admission(c.send()))
                .await
                .unwrap(),
        );
    }
    assert_eq!(
        c.f.persistence
            .pending_commands(c.fence)
            .await
            .unwrap()
            .len(),
        8
    );
    for (index, operation) in operations.iter().enumerate() {
        c.f.persistence
            .accept_observation(
                c.fence,
                &agent(
                    index as u64 + 3,
                    operation.id(),
                    LocalEvidence::Uncertain {},
                ),
            )
            .await
            .unwrap();
    }
    assert!(matches!(
        c.f.persistence
            .enqueue_operation(&c.admission(c.send()))
            .await,
        Err(AppError::Conflict(_))
    ));
    c.f.persistence
        .enqueue_operation(&c.admission(LocalCommand::Inspect {
            session_id: c.session.id,
            thread_id: CloudroomThreadId::try_from("thr_frozen".to_owned()).unwrap(),
        }))
        .await
        .unwrap();
    let resolved = operations[0].id();
    c.f.persistence
        .accept_observation(c.fence, &agent(11, resolved, LocalEvidence::Receipt {}))
        .await
        .unwrap();
    c.f.persistence
        .accept_observation(
            c.fence,
            &agent(12, resolved, LocalEvidence::SubmissionIntent {}),
        )
        .await
        .unwrap();
    c.f.persistence
        .accept_observation(
            c.fence,
            &turn(
                13,
                resolved,
                "turn_resolved",
                TurnState::Completed,
                Some("resolved"),
            ),
        )
        .await
        .unwrap();
    c.f.persistence
        .enqueue_operation(&c.admission(c.send()))
        .await
        .expect("known native completion releases exactly one effect slot");
}

#[tokio::test]
async fn cloudroom_commands_capacity_sql_matches_command_kind_and_terminal_evidence_matrix() {
    let Some(c) = Context::new().await else {
        return;
    };
    let thread = CloudroomThreadId::try_from("thr_frozen".to_owned()).unwrap();
    let commands = [
        c.send(),
        LocalCommand::Create {
            session_id: c.session.id,
            selection: c.session.profile.selection.clone(),
            prompt: LocalPrompt::try_from("capacity matrix".to_owned()).unwrap(),
        },
        LocalCommand::Inspect {
            session_id: c.session.id,
            thread_id: thread.clone(),
        },
        LocalCommand::StopThread {
            session_id: c.session.id,
            thread_id: thread.clone(),
        },
        LocalCommand::ArchiveThread {
            session_id: c.session.id,
            thread_id: thread,
        },
        LocalCommand::RefreshCatalog {},
    ];
    for command in commands {
        for outcome in [
            None,
            Some("succeeded"),
            Some("failed"),
            Some("cancelled"),
            Some("expired"),
            Some("uncertain"),
        ] {
            for state in [
                None,
                Some("running"),
                Some("completed"),
                Some("failed"),
                Some("stopped"),
                Some("missing"),
            ] {
                assert_capacity_case(&c, &command, outcome, state).await;
            }
        }
    }
}

async fn assert_capacity_case(
    c: &Context,
    command: &LocalCommand,
    outcome: Option<&str>,
    state: Option<&str>,
) {
    let admission = c.admission(command.clone());
    let operation = &admission.operation;
    let mut tx = c.f.database.pool.begin().await.unwrap();
    sqlx::query("INSERT INTO local_agent_operations(company_id,id,machine_id,installation_id,session_id,logical_key,command,payload_digest,deadline,outcome,observed_turn_state) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)")
        .bind(c.f.company).bind(operation.id().as_uuid()).bind(c.paired.receipt.machine.id.as_uuid()).bind(c.paired.receipt.machine.installation_id.as_uuid())
        .bind(command_session(command).map(SessionId::as_uuid)).bind(operation.id().to_string()).bind(encode(command).unwrap())
        .bind(operation.payload_digest().as_str()).bind(operation.deadline()).bind(outcome).bind(state).execute(&mut *tx).await.unwrap();
    let counts = capacity(&mut tx, c.f.company, c.paired.receipt.machine.id)
        .await
        .unwrap();
    let row: OperationDb = sqlx::query_as(&format!(
        "SELECT {} FROM local_agent_operations WHERE company_id=$1 AND id=$2",
        operation_columns()
    ))
    .bind(c.f.company)
    .bind(operation.id().as_uuid())
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let known_terminal = matches!(state, Some("completed" | "failed" | "stopped"));
    let outstanding = outcome.is_none()
        || (outcome == Some("uncertain") && command.is_effect() && !known_terminal);
    assert_eq!(
        row.outstanding, outstanding,
        "derived delivery/Ready state agrees"
    );
    assert_eq!(
        counts.queued,
        i64::from(outstanding),
        "{command:?} {outcome:?} {state:?}"
    );
    assert_eq!(
        counts.unsettled,
        i64::from(outstanding && command.is_effect()),
        "{command:?} {outcome:?} {state:?}"
    );
    tx.rollback().await.unwrap();
}
