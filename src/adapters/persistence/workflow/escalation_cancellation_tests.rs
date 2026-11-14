use super::*;

async fn configured(people: PeopleFixture) -> GuardFixture {
    Box::pin(GuardFixture::admit(people, Some(policy()), None)).await
}

async fn current(f: &GuardFixture) -> EscalationDecisionId {
    let decision: Uuid = sqlx::query_scalar(
        "SELECT current_decision_id FROM workflow_escalation_progress WHERE company_id=$1",
    )
    .bind(f.scope.company.as_uuid())
    .fetch_one(f.p().pool())
    .await
    .unwrap();
    EscalationDecisionId::new(decision)
}

#[tokio::test]
async fn workflow_escalation_decision_live_run_cancel_uses_actual_wait_job_and_run_cancellation() {
    let people = Box::pin(PeopleFixture::with_tracking(
        1,
        json!(["reviewer"]),
        ReviewerTracking::FollowRole,
    ))
    .await;
    let f = Box::pin(configured(people)).await;
    assert!(
        f.p()
            .park_decision(f.scope, &notices(f.p()))
            .await
            .unwrap()
            .is_some()
    );
    crate::adapters::persistence::test_support::department_use_cases(f.p())
        .clear_role(
            f.people.role.admission.binding.target.actor.user_id(),
            f.scope.company.as_uuid(),
            f.people.role.department,
            DepartmentRole::Reviewer,
            f.people.principals[0],
        )
        .await
        .unwrap();
    assert_eq!(
        f.worker().drain_once().await.unwrap(),
        2,
        "one withdrawn ordinary reviewer notice and one escalation control"
    );
    let decision = current(&f).await;
    let owner = f.people.role.admission.binding.target.actor.user_id();
    let revision = view(&f, decision, owner).await.cancel_revision.unwrap();
    let wrong = command(
        &f,
        decision,
        owner,
        "stale-cancel",
        EscalationChoice::Cancel {
            expected_run_revision: RunRevision(revision.0 + 1),
        },
    );
    assert!(matches!(
        f.p().submit_escalation(wrong).await,
        Err(AppError::Conflict(_))
    ));
    let request = command(
        &f,
        decision,
        owner,
        "live-cancel",
        EscalationChoice::Cancel {
            expected_run_revision: revision,
        },
    );
    assert_eq!(
        f.p().submit_escalation(request.clone()).await.unwrap(),
        EscalationSubmission::Applied
    );
    assert_eq!(
        f.p().submit_escalation(request).await.unwrap(),
        EscalationSubmission::Replayed
    );
    let result:Value=sqlx::query_scalar("SELECT jsonb_build_object('run',run.state, \
        'waiting',(SELECT count(*) FROM workflow_waits WHERE company_id=$1 AND run_id=$2 AND state='waiting'), \
        'jobs',(SELECT count(*) FROM background_tasks AS job JOIN workflow_executions AS execution \
            ON execution.company_id=job.company_id AND execution.id=job.workflow_execution_id \
            WHERE execution.company_id=$1 AND execution.run_id=$2 AND job.status IN ('pending','processing')), \
        'audit',(SELECT count(*) FROM workflow_run_events WHERE company_id=$1 AND run_id=$2 AND event_kind='control_cancel'), \
        'settlement',(SELECT settlement FROM workflow_escalation_commands WHERE company_id=$1 AND command_key='live-cancel')) \
        FROM workflow_runs AS run WHERE run.company_id=$1 AND run.id=$2")
        .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid()).fetch_one(f.p().pool()).await.unwrap();
    assert_eq!(
        result,
        json!({"run":"cancelled","waiting":0,"jobs":0,"audit":1,"settlement":"cancelled"})
    );
}

#[tokio::test]
async fn workflow_escalation_decision_admission_refusal_has_no_run_control_and_can_be_acknowledged()
{
    let people = Box::pin(PeopleFixture::new(1)).await;
    let binding = &people.role.admission.binding;
    let mut request = binding.request(Some(binding.state().await.revision));
    request.association = RelatedAssociation::Channel(RelatedChannelId::new(people.role.channel));
    request.escalation = Some(policy());
    binding.service().configure(request).await.unwrap();
    let prepared = people.role.prepared("no-run-administration").await;
    crate::adapters::persistence::test_support::department_use_cases(people.role.p())
        .clear_role(
            binding.target.actor.user_id(),
            people.role.company().as_uuid(),
            people.role.department,
            DepartmentRole::Reviewer,
            people.principals[0],
        )
        .await
        .unwrap();
    assert!(matches!(
        people.role.p().admit(&prepared).await,
        Err(AppError::WorkflowRole(_))
    ));
    let p = people.role.p();
    let company = people.role.company();
    let actor = binding.target.actor;
    let worker =
        crate::adapters::persistence::test_support::notification_worker(p, Default::default());
    assert_eq!(worker.drain_once().await.unwrap(), 1);
    let decision: Uuid = sqlx::query_scalar(
        "SELECT current_decision_id FROM workflow_escalation_progress WHERE company_id=$1",
    )
    .bind(company.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    let decision = EscalationDecisionId::new(decision);
    let view = p.read_escalation(company, actor, decision).await.unwrap();
    assert!(matches!(
        view.source,
        crate::application::workflow::escalation::EscalationSourceView::Admission { .. }
    ));
    assert!(view.can_acknowledge && view.cancel_revision.is_none());
    let mut request = SubmitEscalation {
        company,
        actor,
        decision,
        command: DecisionCommandKey::parse("no-run").unwrap(),
        choice: EscalationChoice::Cancel {
            expected_run_revision: RunRevision(0),
        },
    };
    let error = p.submit_escalation(request.clone()).await.unwrap_err();
    assert!(
        matches!(error,AppError::BadRequest(ref code) if code=="escalation.no_run_to_cancel"),
        "{error:?}"
    );
    request.choice = EscalationChoice::Acknowledge;
    assert_eq!(
        p.submit_escalation(request).await.unwrap(),
        EscalationSubmission::Applied
    );
    let runs: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_runs WHERE company_id=$1")
        .bind(company.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(runs, 0);
}

#[tokio::test]
async fn workflow_escalation_decision_restricted_source_keeps_safe_history_and_denies_new_commands()
{
    let f = Box::pin(GuardFixture::new(Some(policy()))).await;
    let actor = f.people.users[0];
    sqlx::query("UPDATE company_members SET access_level=5 WHERE company_id=$1 AND user_id=$2")
        .bind(f.scope.company.as_uuid())
        .bind(actor)
        .execute(f.p().pool())
        .await
        .unwrap();
    let mut tx = f.p().pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(f.scope.company.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(f.scope.company.as_uuid())
        .bind(f.scope.run.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    crate::adapters::persistence::workflow::knowledge::raise_content_level_on(
        &mut tx,
        f.scope.company,
        f.scope.run,
        crate::entities::access_level::AccessLevel::L5,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        f.p().fail_run_on_guard(f.failure()).await.unwrap(),
        GuardFailureOutcome::Failed
    );
    assert_eq!(f.worker().drain_once().await.unwrap(), 1);
    assert_eq!(f.worker().drain_once().await.unwrap(), 1);
    let decision = current(&f).await;
    assert!(view(&f, decision, actor).await.can_acknowledge);
    sqlx::query("UPDATE company_members SET access_level=0 WHERE company_id=$1 AND user_id=$2")
        .bind(f.scope.company.as_uuid())
        .bind(actor)
        .execute(f.p().pool())
        .await
        .unwrap();
    let restricted = view(&f, decision, actor).await;
    assert!(restricted.payload_restricted);
    assert!(
        !restricted.can_acknowledge
            && !restricted.can_reassign
            && restricted.cancel_revision.is_none()
    );
    assert!(restricted.assignment_options.is_empty());
    let html = crate::adapters::http::pages::workflow_escalation_page(&restricted);
    assert!(
        html.contains("Source content is restricted.")
            && !html.contains(&f.scope.run.as_uuid().to_string())
    );
    assert!(
        f.p()
            .submit_escalation(command(
                &f,
                decision,
                actor,
                "restricted",
                EscalationChoice::Acknowledge
            ))
            .await
            .is_err()
    );
    assert_eq!(facts(&f).await["commands"], 0);
}

#[tokio::test]
async fn workflow_escalation_decision_database_refuses_an_unassigned_current_member_receipt() {
    let (f, decision) = Box::pin(pending(1)).await;
    let (user, principal) = eligible_member(&f).await;
    let mut tx = f.p().pool().begin().await.unwrap();
    sqlx::query("UPDATE workflow_escalation_decisions SET settled_at=clock_timestamp(),settlement='acknowledged' WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company.as_uuid()).bind(decision.as_uuid()).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE workflow_escalation_progress SET state='settled' WHERE company_id=$1")
        .bind(f.scope.company.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    let error=sqlx::query("INSERT INTO workflow_escalation_commands \
        (company_id,command_key,case_id,decision_id,actor_user_id,actor_principal_id,fingerprint,choice,settlement) \
        SELECT company_id,'forged-actor',case_id,id,$3,$4,repeat('0',64),'{\"kind\":\"acknowledge\"}'::jsonb,'acknowledged' \
        FROM workflow_escalation_decisions WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company.as_uuid()).bind(decision.as_uuid()).bind(user).bind(principal.as_uuid()).execute(&mut *tx).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("workflow_escalation_command_guard")
    );
    tx.rollback().await.unwrap();
    assert!(view(&f, decision, f.people.users[0]).await.can_acknowledge);
}
