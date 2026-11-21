//! Real invitation and account deletion compete behind their production lock prefixes.
use super::*;
use crate::{
    adapters::persistence::test_support::wait_until_backends_are_blocked,
    application::ports::account_deletion::HostAccountDeletion,
};
use std::time::Duration;
use tokio::task::JoinSet;

#[derive(Clone, Copy)]
enum First {
    Acceptance,
    Deletion,
}

#[derive(Debug)]
enum Outcome {
    Acceptance(bool),
    Deletion,
}

async fn finish(tasks: &mut JoinSet<Result<Outcome, AppError>>) -> Vec<Outcome> {
    let mut outcomes = Vec::new();
    while !tasks.is_empty() {
        match tokio::time::timeout(Duration::from_secs(15), tasks.join_next()).await {
            Ok(Some(Ok(Ok(outcome)))) => outcomes.push(outcome),
            failure => {
                tasks.abort_all();
                while tasks.join_next().await.is_some() {}
                panic!("root membership competitors did not complete: {failure:?}");
            }
        }
    }
    outcomes
}

fn start(
    tasks: &mut JoinSet<Result<Outcome, AppError>>,
    f: &OwnedCompany,
    user: &crate::entities::user::User,
    invite: Uuid,
    operation: First,
) {
    let persistence = f.persistence.clone();
    let user = user.clone();
    tasks.spawn(async move {
        match operation {
            First::Acceptance => persistence
                .accept_pending_invite(invite, user.id, &user.email)
                .await
                .map(|accepted| Outcome::Acceptance(accepted.is_some())),
            First::Deletion => persistence
                .delete_user_account(user.id)
                .await
                .map(|()| Outcome::Deletion),
        }
    });
}

async fn assert_removed(
    f: &OwnedCompany,
    other: &OwnedCompany,
    user: &crate::entities::user::User,
    original: &Membership,
) {
    let remaining: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM users WHERE id=$1) + \
                (SELECT count(*) FROM department_members WHERE root_user_id=$1) + \
                (SELECT count(*) FROM principals WHERE user_id=$1) + \
                (SELECT count(*) FROM department_roles AS holder JOIN principals AS principal \
                   ON principal.company_id=holder.company_id AND principal.id=holder.principal_id \
                   WHERE principal.historical_person_user_id=$1) + \
                (SELECT count(*) FROM addresses AS address JOIN principals AS principal \
                   ON principal.company_id=address.company_id AND principal.id=address.principal_id \
                   WHERE principal.historical_person_user_id=$1)",
    )
    .bind(user.id)
    .fetch_one(f.persistence.pool())
    .await
    .unwrap();
    assert_eq!(
        remaining, 0,
        "deletion leaves no live membership, roles or addresses"
    );
    let retired: String = sqlx::query_scalar("SELECT kind FROM principals WHERE id=$1")
        .bind(original.principal_id)
        .fetch_one(f.persistence.pool())
        .await
        .unwrap();
    assert_eq!(
        retired, "external",
        "account removal preserves the lifetime principal"
    );
    for company in [f.company_id, other.company_id] {
        assert!(
            f.persistence
                .company_access(user.id, company)
                .await
                .unwrap()
                .is_none()
        );
    }
}

async fn competing(first: First) {
    let Some(database) = own_database().await else {
        return;
    };
    let f = OwnedCompany::new(database.pool.clone()).await;
    let other = OwnedCompany::new(database.pool.clone()).await;
    let user = other.member("racing", CompanyAccessRole::Member).await;
    let original = membership(&other, user.id).await;
    let invite = f
        .persistence
        .create_invite(
            crate::adapters::persistence::test_support::invite_command(
                &f.persistence,
                f.company_id,
                &user.email,
                CompanyAccessRole::Admin,
            )
            .await,
        )
        .await
        .unwrap();
    // The first operation owns the account before blocking on a company. The second
    // must then wait on that account, proving both production serialization orders.
    let gated_company = match first {
        First::Acceptance => f.company_id,
        First::Deletion => other.company_id,
    };
    let mut barrier = database.pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR UPDATE")
        .bind(gated_company)
        .fetch_one(&mut *barrier)
        .await
        .unwrap();
    let mut tasks = JoinSet::new();
    start(&mut tasks, &f, &user, invite.id, first);
    wait_until_backends_are_blocked(&database.pool, 1).await;
    let second = match first {
        First::Acceptance => First::Deletion,
        First::Deletion => First::Acceptance,
    };
    start(&mut tasks, &f, &user, invite.id, second);
    wait_until_backends_are_blocked(&database.pool, 2).await;
    barrier.commit().await.unwrap();
    let outcomes = finish(&mut tasks).await;
    let accepted = outcomes
        .iter()
        .find_map(|outcome| match outcome {
            Outcome::Acceptance(accepted) => Some(*accepted),
            Outcome::Deletion => None,
        })
        .unwrap();
    assert_eq!(accepted, matches!(first, First::Acceptance));
    assert!(
        outcomes
            .iter()
            .any(|outcome| matches!(outcome, Outcome::Deletion))
    );
    assert_removed(&f, &other, &user, &original).await;
    let pending = f
        .persistence
        .get_invite_by_id(f.company_id, invite.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        pending.status,
        if accepted { "accepted" } else { "pending" }
    );
}

#[tokio::test]
async fn root_competing_invite_first_account_deletion_cleans_the_new_root_membership() {
    competing(First::Acceptance).await;
}

#[tokio::test]
async fn root_competing_account_deletion_first_invite_cannot_restore_membership() {
    competing(First::Deletion).await;
}
