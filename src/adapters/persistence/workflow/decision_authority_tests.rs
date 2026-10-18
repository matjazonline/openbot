use super::*;
use crate::adapters::persistence::test_support::wait_until_a_backend_is_blocked;
use crate::use_cases::channel::{ChannelPersistence, ChannelWrite};

async fn channel_fixture() -> (DecisionFixture, Uuid) {
    let mut f = Box::pin(DecisionFixture::new(1, None)).await;
    let channel = ChannelPersistence::create(
        f.persistence(),
        f.scope.company.as_uuid(),
        ChannelWrite {
            name: "Review".into(),
            slug: "review".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    sqlx::query("UPDATE channels SET access_mode='allowlist' WHERE id=$1")
        .bind(channel)
        .execute(f.persistence().pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO channel_principal_grants(company_id,channel_id,principal_id,capability,provenance) SELECT $1,$2,id,'view','configured_allowlist' FROM principals WHERE company_id=$1 AND user_id=$3")
        .bind(f.scope.company.as_uuid()).bind(channel).bind(f.users[0]).execute(f.persistence().pool()).await.unwrap();
    f.admission.binding.target.binding = WorkflowBindingId::new(Uuid::new_v4());
    let binding = &f.admission.binding;
    let mut request = binding.request(None);
    request.association = RelatedAssociation::Channel(RelatedChannelId::new(channel));
    let configured = binding.service().configure(request).await.unwrap();
    binding
        .service()
        .activate(SetBindingActivity {
            target: binding.target,
            expected: configured.revision,
        })
        .await
        .unwrap();
    let mut request = f.admission.request("channel", f.admission.manual());
    request.association = RelatedAssociation::Channel(RelatedChannelId::new(channel));
    let command = f.admission.prepare(request).await;
    f.persistence().admit(&command).await.unwrap();
    let job = sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id=$1")
        .bind(command.first_execution_id().as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    f.scope = ActivationRequest {
        company: command.company_id(),
        run: command.proposed_run_id(),
        execution: command.first_execution_id(),
        job: WorkflowJobId(job),
    };
    (f, channel)
}

async fn hidden(f: &DecisionFixture, view: &DecisionView, actor: WorkflowActor) {
    let before = f.counts().await;
    let mut submit = f.submit(view, 0, "hidden");
    submit.actor = actor;
    assert!(matches!(
        f.persistence().submit_decision(submit).await,
        Err(AppError::NotFound(_))
    ));
    assert!(matches!(
        f.persistence()
            .read_decision(f.scope.company, actor, view.decision)
            .await,
        Err(AppError::NotFound(_))
    ));
    let comment = AddDecisionComment {
        company: f.scope.company,
        actor,
        decision: view.decision,
        command_key: DecisionCommandKey::parse("hidden-comment").unwrap(),
        body: "hidden".into(),
    };
    assert!(matches!(
        f.persistence().add_decision_comment(comment).await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(f.counts().await, before);
    let submission: Option<String> =
        sqlx::query_scalar("SELECT choice FROM workflow_decisions WHERE id=$1")
            .bind(view.decision.0)
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    assert!(submission.is_none());
}

#[tokio::test]
async fn workflow_decision_nonmember_and_removed_membership_are_hidden() {
    let f = Box::pin(DecisionFixture::new(1, None)).await;
    let view = f.park().await;
    let outsider = f
        .persistence()
        .create_user("outside", "outside@example.test", "hash")
        .await
        .unwrap();
    hidden(
        &f,
        &view,
        WorkflowActor::authenticated(outsider.id).unwrap(),
    )
    .await;
    sqlx::query("DELETE FROM company_members WHERE company_id=$1 AND user_id=$2")
        .bind(f.scope.company.as_uuid())
        .bind(f.users[0])
        .execute(f.persistence().pool())
        .await
        .unwrap();
    hidden(&f, &view, f.actor(0)).await;
}

#[derive(Clone, Copy)]
enum Revocation {
    Membership,
    Visibility(Uuid),
}
async fn revoke(tx: &mut Transaction<'_, Postgres>, f: &DecisionFixture, kind: Revocation) {
    match kind {
        Revocation::Membership => {
            sqlx::query("DELETE FROM company_members WHERE company_id=$1 AND user_id=$2")
                .bind(f.scope.company.as_uuid())
                .bind(f.users[0])
                .execute(&mut **tx)
                .await
                .unwrap();
        }
        Revocation::Visibility(channel) => {
            sqlx::query(
                "DELETE FROM channel_principal_grants WHERE company_id=$1 AND channel_id=$2",
            )
            .bind(f.scope.company.as_uuid())
            .bind(channel)
            .execute(&mut **tx)
            .await
            .unwrap();
        }
    }
}

async fn revocation_first(f: &DecisionFixture, view: &DecisionView, kind: Revocation) {
    let before = f.counts().await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    revoke(&mut tx, f, kind).await;
    let command = f.submit(view, 0, "revocation-first");
    let (result, ()) = tokio::join!(f.persistence().submit_decision(command), async {
        wait_until_a_backend_is_blocked(f.persistence().pool()).await;
        tx.commit().await.unwrap();
    });
    assert!(matches!(result, Err(AppError::NotFound(_))));
    assert_eq!(f.counts().await, before);
    hidden(f, view, f.actor(0)).await;
}

async fn submission_first(f: &DecisionFixture, view: &DecisionView, kind: Revocation) {
    let mut blocker = f.persistence().pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM background_tasks WHERE id=$1 FOR UPDATE")
        .bind(f.scope.job.0)
        .execute(&mut *blocker)
        .await
        .unwrap();
    let p = f.persistence().clone();
    let command = f.submit(view, 0, "submission-first");
    let submit = tokio::spawn(async move { p.submit_decision(command).await });
    // The command reaches the job only after locking membership and visibility.
    wait_until_a_backend_is_blocked(f.persistence().pool()).await;
    let mut revoker = f.persistence().pool().begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *revoker)
        .await
        .unwrap();
    let ((), ()) = tokio::join!(
        async {
            revoke(&mut revoker, f, kind).await;
            revoker.commit().await.unwrap();
        },
        async {
            tokio::time::timeout(std::time::Duration::from_millis(800), async {
                loop {
                    let blocked: bool =
                        sqlx::query_scalar("SELECT cardinality(pg_blocking_pids($1))>0")
                            .bind(pid)
                            .fetch_one(f.persistence().pool())
                            .await
                            .unwrap();
                    if blocked {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap();
            blocker.commit().await.unwrap();
        }
    );
    assert_eq!(
        submit.await.unwrap().unwrap().disposition,
        DecisionDisposition::Applied
    );
    assert!(matches!(
        f.persistence()
            .read_decision(f.scope.company, f.actor(0), view.decision)
            .await,
        Err(AppError::NotFound(_))
    ));
}

#[tokio::test]
async fn workflow_decision_membership_revocation_serializes_in_both_orders() {
    let f = Box::pin(DecisionFixture::new(1, None)).await;
    let view = f.park().await;
    revocation_first(&f, &view, Revocation::Membership).await;
    let f = Box::pin(DecisionFixture::new(1, None)).await;
    let view = f.park().await;
    submission_first(&f, &view, Revocation::Membership).await;
}

#[tokio::test]
async fn workflow_decision_visibility_revocation_serializes_in_both_orders() {
    let (f, channel) = Box::pin(channel_fixture()).await;
    let view = f.park().await;
    revocation_first(&f, &view, Revocation::Visibility(channel)).await;
    let (f, channel) = Box::pin(channel_fixture()).await;
    let view = f.park().await;
    submission_first(&f, &view, Revocation::Visibility(channel)).await;
}

async fn history(f: &DecisionFixture, view: &DecisionView) -> DecisionView {
    f.persistence()
        .read_decision(
            f.scope.company,
            f.admission.binding.target.actor,
            view.decision,
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn workflow_decision_production_removal_preserves_open_and_settled_history() {
    use crate::use_cases::company_invite::CompanyInvitePersistence;
    for settled in [false, true] {
        let f = Box::pin(DecisionFixture::new(1, None)).await;
        let view = f.park().await;
        f.persistence()
            .add_decision_comment(AddDecisionComment {
                company: f.scope.company,
                actor: f.actor(0),
                decision: view.decision,
                command_key: DecisionCommandKey::parse("history").unwrap(),
                body: "review discussion".into(),
            })
            .await
            .unwrap();
        if settled {
            f.persistence()
                .submit_decision(f.submit(&view, 0, "history"))
                .await
                .unwrap();
        }
        let before = history(&f, &view).await;
        f.persistence()
            .remove_member(f.scope.company.as_uuid(), f.users[0])
            .await
            .unwrap();
        assert_eq!(history(&f, &view).await, before);
        assert!(matches!(
            f.persistence()
                .read_decision(f.scope.company, f.actor(0), view.decision)
                .await,
            Err(AppError::NotFound(_))
        ));
        if !settled {
            hidden(&f, &view, f.actor(0)).await;
        }
    }
}

async fn await_blocked_removal(pool: &sqlx::PgPool) {
    tokio::time::timeout(std::time::Duration::from_millis(800), async {
        loop {
            let blocked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND query LIKE 'UPDATE principals%' AND cardinality(pg_blocking_pids(pid))>0)")
                .fetch_one(pool).await.unwrap();
            if blocked { break; }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }).await.unwrap();
}

#[tokio::test]
async fn workflow_decision_submission_blocks_actual_removal_until_commit() {
    use crate::use_cases::company_invite::CompanyInvitePersistence;
    let f = Box::pin(DecisionFixture::new(1, None)).await;
    let view = f.park().await;
    let mut blocker = f.persistence().pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM background_tasks WHERE id=$1 FOR UPDATE")
        .bind(f.scope.job.0)
        .execute(&mut *blocker)
        .await
        .unwrap();
    let p = f.persistence().clone();
    let command = f.submit(&view, 0, "actual-removal");
    let submit = tokio::spawn(async move { p.submit_decision(command).await });
    wait_until_a_backend_is_blocked(f.persistence().pool()).await;
    let (p, company, user) = (
        f.persistence().clone(),
        f.scope.company.as_uuid(),
        f.users[0],
    );
    let removal = tokio::spawn(async move { p.remove_member(company, user).await });
    await_blocked_removal(f.persistence().pool()).await;
    blocker.commit().await.unwrap();
    assert_eq!(
        submit.await.unwrap().unwrap().disposition,
        DecisionDisposition::Applied
    );
    removal.await.unwrap().unwrap();
    let read = history(&f, &view).await;
    assert_eq!(read.reviewers, f.users);
    assert!(read.submission.is_some());
}

#[tokio::test]
async fn workflow_decision_principal_conversion_before_submit_refuses() {
    let f = Box::pin(DecisionFixture::new(1, None)).await;
    let view = f.park().await;
    let mut removal = f.persistence().pool().begin().await.unwrap();
    // The production port's exact mutation order, paused before its commit.
    sqlx::query(
        "UPDATE principals SET kind='external',user_id=NULL WHERE company_id=$1 AND user_id=$2",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.users[0])
    .execute(&mut *removal)
    .await
    .unwrap();
    revoke(&mut removal, &f, Revocation::Membership).await;
    let (result, ()) = tokio::join!(
        f.persistence()
            .submit_decision(f.submit(&view, 0, "converted")),
        async {
            wait_until_a_backend_is_blocked(f.persistence().pool()).await;
            removal.commit().await.unwrap();
        }
    );
    assert!(matches!(result, Err(AppError::NotFound(_))));
    hidden(&f, &view, f.actor(0)).await;
    assert_eq!(history(&f, &view).await.reviewers, f.users);
}

#[tokio::test]
async fn workflow_decision_park_blocks_actual_removal_until_notice_commit() {
    use crate::use_cases::company_invite::CompanyInvitePersistence;
    let f = Box::pin(DecisionFixture::new(1, None)).await;
    let mut blocker = f.persistence().pool().begin().await.unwrap();
    sqlx::query("LOCK TABLE workflow_decisions IN SHARE MODE")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let (p, scope, notices) = (f.persistence().clone(), f.scope, f.notices.clone());
    let park = tokio::spawn(async move { p.park_decision(scope, &notices).await });
    wait_until_a_backend_is_blocked(f.persistence().pool()).await;
    let (p, company, user) = (
        f.persistence().clone(),
        f.scope.company.as_uuid(),
        f.users[0],
    );
    let removal = tokio::spawn(async move { p.remove_member(company, user).await });
    await_blocked_removal(f.persistence().pool()).await;
    blocker.commit().await.unwrap();
    assert!(park.await.unwrap().unwrap().is_some());
    removal.await.unwrap().unwrap();
    let decision = sqlx::query_scalar("SELECT id FROM workflow_decisions WHERE run_id=$1")
        .bind(f.scope.run.as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    let read = f
        .persistence()
        .read_decision(
            f.scope.company,
            f.admission.binding.target.actor,
            DecisionId(decision),
        )
        .await
        .unwrap();
    assert_eq!(read.reviewers, f.users);
}

#[tokio::test]
async fn workflow_decision_removal_before_park_settles_unavailable() {
    use crate::use_cases::company_invite::CompanyInvitePersistence;
    let f = Box::pin(DecisionFixture::new(1, None)).await;
    f.persistence()
        .remove_member(f.scope.company.as_uuid(), f.users[0])
        .await
        .unwrap();
    assert!(
        f.persistence()
            .park_decision(f.scope, &f.notices)
            .await
            .unwrap()
            .is_none()
    );
    assert_failed_without_park(&f).await;
}

#[tokio::test]
async fn workflow_decision_membership_delete_preserves_settled_history() {
    let f = Box::pin(DecisionFixture::new(1, None)).await;
    let view = f.park().await;
    f.persistence()
        .add_decision_comment(AddDecisionComment {
            company: f.scope.company,
            actor: f.actor(0),
            decision: view.decision,
            command_key: DecisionCommandKey::parse("raw-history").unwrap(),
            body: "history".into(),
        })
        .await
        .unwrap();
    f.persistence()
        .submit_decision(f.submit(&view, 0, "raw-history"))
        .await
        .unwrap();
    let before = history(&f, &view).await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    revoke(&mut tx, &f, Revocation::Membership).await;
    tx.commit().await.unwrap();
    assert_eq!(history(&f, &view).await, before);
}

#[tokio::test]
async fn workflow_decision_park_loses_to_inflight_member_removal() {
    for convert in [false, true] {
        let f = Box::pin(DecisionFixture::new(1, None)).await;
        let mut removal = f.persistence().pool().begin().await.unwrap();
        if convert {
            sqlx::query("UPDATE principals SET kind='external',user_id=NULL WHERE company_id=$1 AND user_id=$2")
                .bind(f.scope.company.as_uuid()).bind(f.users[0]).execute(&mut *removal).await.unwrap();
        }
        revoke(&mut removal, &f, Revocation::Membership).await;
        let (parked, ()) =
            tokio::join!(f.persistence().park_decision(f.scope, &f.notices), async {
                wait_until_a_backend_is_blocked(f.persistence().pool()).await;
                removal.commit().await.unwrap();
            });
        assert!(parked.unwrap().is_none());
        assert_failed_without_park(&f).await;
    }
}
