//! Real ingress competes with identity/account/target writers; database aborts are never an oracle.
use super::*;
use crate::{
    application::ports::thread::qualified_email_identity,
    entities::{access_level::AccessLevel, company_member::CompanyAccessRole},
    use_cases::{
        company_invite::CreateCompanyInvite,
        thread::{InboundIngestResult, PreparedInbound},
        user::{
            AccountChange, AccountChangeKind, AccountChangePersistence, PendingAccountChange,
            auth_attempt_key,
        },
    },
};
use futures::future::{BoxFuture, LocalBoxFuture};
use std::time::Duration;
use tokio::task::JoinSet;

#[derive(Clone, Copy, PartialEq, Eq)]
enum First {
    Mail,
    Change,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Change {
    Rekey,
    Promote,
    Rename,
    Remove,
}
enum Outcome {
    Mail(Box<crate::app_error::AppResult<InboundIngestResult>>),
    Change,
}
struct State {
    f: Fixture,
    user: Uuid,
    target: Uuid,
    input: InboundMessage,
    new: EmailAddress,
    old: String,
}
struct Contenders {
    state: State,
    mail: BoxFuture<'static, Outcome>,
    mutation: BoxFuture<'static, Outcome>,
}

fn setup(change: Change) -> LocalBoxFuture<'static, Option<Contenders>> {
    Box::pin(async move {
        let f = Fixture::isolated().await?;
        let (user, name) = account(&f.persistence, "identity_race").await;
        sqlx::query("UPDATE users SET email_verified_at=CURRENT_TIMESTAMP WHERE id=$1")
            .bind(user)
            .execute(f.pool())
            .await
            .unwrap();
        let old = email_of(f.pool(), user).await;
        let new = EmailAddress::from(format!("changed-{}@example.test", Uuid::new_v4().simple()));
        let target = if change == Change::Promote {
            production_agent(&f, true).await
        } else {
            f.join(user, &name).await
        };
        let service = Arc::new(threads(&f));
        let input = email(&f, target, &old, "identity-race", "identity-root").await;
        let InboundPreflight::Accepted(prepared) =
            service.preflight_inbound(input.clone()).await.unwrap()
        else {
            panic!("initial current identity must pass preflight");
        };
        let invite = if change == Change::Promote {
            Some(
                f.persistence
                    .create_invite(CreateCompanyInvite {
                        actor_id: f.owner,
                        company_id: f.company.id,
                        email: old.clone().into(),
                        role: CompanyAccessRole::Member,
                        access_level: AccessLevel::try_from(1_i16).unwrap(),
                    })
                    .await
                    .unwrap()
                    .id,
            )
        } else {
            None
        };
        if change == Change::Rekey {
            f.persistence
                .save_pending_account_change(PendingAccountChange {
                    user_id: user,
                    change: AccountChange::Email(&new),
                    confirmation_code_hash: "race-code",
                    expires_at: chrono::Utc::now() + chrono::Duration::minutes(15),
                })
                .await
                .unwrap();
        }
        let state = State {
            f,
            user,
            target,
            input,
            new,
            old,
        };
        Some(Contenders {
            mutation: mutation(&state, change, invite),
            state,
            mail: mail(service, prepared),
        })
    })
}

fn mail(
    service: Arc<ThreadUseCases>,
    prepared: Box<PreparedInbound>,
) -> BoxFuture<'static, Outcome> {
    // Erase the production transaction future before Tokio adds its task/drop machinery.
    Box::pin(
        async move { Outcome::Mail(Box::new(service.commit_prepared_inbound(*prepared).await)) },
    )
}

fn mutation(state: &State, change: Change, invite: Option<Uuid>) -> BoxFuture<'static, Outcome> {
    let store = state.f.persistence.clone();
    let company = state.f.company.id;
    let removing_owner = state.f.owner;
    let user = state.user;
    let old = state.old.clone();
    let next = state.new.clone();
    Box::pin(async move {
        match change {
            Change::Rekey => {
                let key =
                    auth_attempt_key("direct-race", "account-change-code:v1", &[user.as_bytes()]);
                let result = store
                    .confirm_pending_account_change(
                        user,
                        AccountChangeKind::Email,
                        "race-code",
                        &key,
                    )
                    .await
                    .expect("real confirmed rekey must commit")
                    .expect("confirmation consumed");
                assert_eq!(result.email, next.as_str());
                assert!(result.email_verified_at.is_some());
            }
            Change::Promote => {
                assert!(
                    store
                        .accept_pending_invite(invite.unwrap(), user, &old)
                        .await
                        .expect("actual invitation promotion must commit")
                        .is_some()
                );
            }
            Change::Rename => {
                assert!(
                    store
                        .rename_person_address(user, company, &ChannelSlug::from("renamed-person"))
                        .await
                        .expect("rename must commit")
                        .is_some()
                );
            }
            Change::Remove => {
                store
                    .remove_member(crate::use_cases::company_invite::RemoveCompanyMember {
                        actor_id: removing_owner,
                        company_id: company,
                        member_user_id: user,
                    })
                    .await
                    .expect("removal must commit");
            }
        }
        Outcome::Change
    })
}

async fn waiting(f: &Fixture, count: i64) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let actual: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock'")
                .fetch_one(f.pool()).await.unwrap();
            if actual >= count { return; }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.expect("ordered production contenders reach the owned database barrier");
}

async fn barrier(state: &State, change: Change) -> sqlx::Transaction<'_, sqlx::Postgres> {
    let mut tx = state.f.pool().begin().await.unwrap();
    if matches!(change, Change::Rekey | Change::Promote) {
        let identity = qualified_email_identity(state.old.clone()).unwrap();
        crate::adapters::persistence::participant::lock_identity_key(
            &mut tx,
            state.f.company.id,
            &identity,
        )
        .await
        .unwrap();
    } else {
        sqlx::query("SELECT id FROM principals WHERE company_id=$1 AND id=$2 FOR UPDATE")
            .bind(state.f.company.id)
            .bind(state.target)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx
}

async fn finish(tasks: &mut JoinSet<Outcome>) -> Vec<Outcome> {
    let results = tokio::time::timeout(Duration::from_secs(10), async {
        let mut outcomes = Vec::new();
        while let Some(result) = tasks.join_next().await {
            outcomes.push(result.expect("no production panic/abort"));
        }
        outcomes
    })
    .await;
    match results {
        Ok(results) => results,
        Err(_) => {
            tasks.shutdown().await;
            panic!("both contenders must finish without deadlock/timeout victims");
        }
    }
}

fn admitted(results: Vec<Outcome>, first: First, change: Change) -> i64 {
    assert_eq!(results.len(), 2);
    let mut changes = 0;
    let mut admitted = 0;
    for result in results {
        match result {
            Outcome::Change => changes += 1,
            Outcome::Mail(result) => {
                let result = *result;
                if first == First::Change && change != Change::Promote {
                    assert!(
                        matches!(result, Err(AppError::Superseded(_))),
                        "obsolete input must be controlled refusal: {result:?}"
                    );
                } else {
                    assert!(result.expect("current mail must commit normally").accepted);
                    admitted += 1;
                }
            }
        }
    }
    assert_eq!(changes, 1);
    admitted
}

fn assertions(state: State, change: Change, admitted: i64) -> LocalBoxFuture<'static, ()> {
    Box::pin(async move {
        let State {
            f,
            user,
            target,
            input,
            new,
            old,
        } = state;
        let company = f.company.id;
        let counts: (i64,i64,i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM threads WHERE company_id=$1), (SELECT count(*) FROM messages WHERE company_id=$1), (SELECT count(*) FROM workflow_message_intents WHERE company_id=$1), (SELECT count(*) FROM notification_events WHERE company_id=$1)")
            .bind(company).fetch_one(f.pool()).await.unwrap();
        assert_eq!(
            counts,
            (
                admitted,
                admitted,
                admitted,
                if change == Change::Promote {
                    0
                } else {
                    admitted
                }
            )
        );
        if change == Change::Rekey {
            let old_identity: (Uuid, String) = sqlx::query_as("SELECT principal_id,status FROM participant_identities WHERE company_id=$1 AND transport='email' AND subject=$2")
                .bind(company).bind(old).fetch_one(f.pool()).await.unwrap();
            assert_ne!(old_identity.0, target);
            assert_eq!(old_identity.1, "observed");
            if admitted == 1 {
                let author: Uuid = sqlx::query_scalar(
                    "SELECT author_principal_id FROM messages WHERE company_id=$1",
                )
                .bind(company)
                .fetch_one(f.pool())
                .await
                .unwrap();
                assert_eq!(
                    author, old_identity.0,
                    "committed old mail retains the retired identity attribution"
                );
            }
            let refused = threads(&f).ingest(input).await.unwrap();
            assert!(
                !refused.accepted,
                "retired sender never regains personal authority"
            );
            let current = email(&f, target, new.as_str(), "current-identity", "new-root").await;
            assert!(threads(&f).ingest(current).await.unwrap().accepted);
            let current_person: Uuid = sqlx::query_scalar("SELECT principal_id FROM participant_identities WHERE company_id=$1 AND transport='email' AND subject=$2 AND status='verified'")
                .bind(company).bind(new.as_str()).fetch_one(f.pool()).await.unwrap();
            assert_eq!(current_person, target);
        }
        if change == Change::Promote {
            let person = f.principal_of(user).await;
            let author: Uuid =
                sqlx::query_scalar("SELECT author_principal_id FROM messages WHERE company_id=$1")
                    .bind(company)
                    .fetch_one(f.pool())
                    .await
                    .unwrap();
            assert_eq!(
                author, person,
                "promotion preserves canonical attribution in either order"
            );
        }
    })
}

fn competing(first: First, change: Change) -> LocalBoxFuture<'static, ()> {
    Box::pin(async move {
        let Some(Contenders {
            state,
            mail,
            mutation,
        }) = setup(change).await
        else {
            return;
        };
        let gate = barrier(&state, change).await;
        let mut tasks = JoinSet::new();
        match first {
            First::Mail => {
                tasks.spawn(mail);
                waiting(&state.f, 1).await;
                tasks.spawn(mutation);
            }
            First::Change => {
                tasks.spawn(mutation);
                waiting(&state.f, 1).await;
                tasks.spawn(mail);
            }
        }
        waiting(&state.f, 2).await;
        gate.commit().await.unwrap();
        let admitted = admitted(finish(&mut tasks).await, first, change);
        assertions(state, change, admitted).await;
    })
}

#[tokio::test]
async fn direct_competing_self_mail_rekey_finishes_in_both_identity_lock_orders() {
    competing(First::Mail, Change::Rekey).await;
    competing(First::Change, Change::Rekey).await;
}
#[tokio::test]
async fn direct_competing_person_promotion_and_mail_finish_in_both_identity_lock_orders() {
    competing(First::Mail, Change::Promote).await;
    competing(First::Change, Change::Promote).await;
}
#[tokio::test]
async fn direct_competing_person_rename_and_mail_finish_in_both_target_lock_orders() {
    competing(First::Mail, Change::Rename).await;
    competing(First::Change, Change::Rename).await;
}
#[tokio::test]
async fn direct_competing_person_removal_and_mail_finish_in_both_target_lock_orders() {
    competing(First::Mail, Change::Remove).await;
    competing(First::Change, Change::Remove).await;
}
