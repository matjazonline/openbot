use super::*;
use crate::entities::value_objects::EmailAddress;
use crate::use_cases::user::{ExternalIdentity, LoginMethod, ProfileUpdate, UserPersistence};

async fn change_address(f: &FollowFixture) -> EmailAddress {
    let p = f.people.role.p();
    let user = UserPersistence::get_by_id(p, f.people.users[0])
        .await
        .unwrap()
        .unwrap();
    let email = EmailAddress::from(format!("changed-{}@example.test", Uuid::new_v4().simple()));
    let updated = p
        .update_profile(
            user.id,
            ProfileUpdate {
                username: &user.username,
                email: &email,
                avatar_url: None,
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.email, email.as_str());
    assert!(updated.email_verified_at.is_none());
    email
}

fn claim<'a>(subject: &'a str, email: &'a EmailAddress) -> ExternalIdentity<'a> {
    ExternalIdentity {
        provider: LoginMethod::Google,
        subject,
        email,
        email_verified: true,
        display_name: None,
    }
}

#[tokio::test]
async fn workflow_reviewer_follow_actual_profile_address_change_refreshes_lost_verified_visibility()
{
    let f = Box::pin(fixture()).await;
    let proposal = f.proposal().await;
    change_address(&f).await;
    assert!(
        f.assignment().await.reviewers.is_empty(),
        "the actual profile writer must refresh lost proof/visibility"
    );
    assert_eq!(state(&f).await["state"], "pending");
    assert_eq!(f.proposal().await, proposal);
}

#[tokio::test]
async fn workflow_reviewer_follow_actual_external_link_repairs_current_verified_visibility() {
    let f = Box::pin(fixture()).await;
    let email = change_address(&f).await;
    // Synchronize through another real writer, isolating the link's repair obligation
    // even while the profile writer's missing refresh is independently under test.
    edit(&f, None, None).await.unwrap();
    assert!(f.assignment().await.reviewers.is_empty());
    assert_eq!(state(&f).await["state"], "pending");
    let subject = format!("follow-link-{}", Uuid::new_v4().simple());
    f.people
        .role
        .p()
        .link_external_identity(f.people.users[0], claim(&subject, &email))
        .await
        .unwrap();
    assert_eq!(
        f.assignment().await.reviewers.len(),
        1,
        "the real verified link must repair the assignment"
    );
    assert_eq!(state(&f).await["state"], "withdrawn");
}

#[tokio::test]
async fn workflow_reviewer_follow_actual_external_login_repairs_only_current_address_proof() {
    let f = Box::pin(fixture()).await;
    let user = UserPersistence::get_by_id(f.people.role.p(), f.people.users[0])
        .await
        .unwrap()
        .unwrap();
    let old_email = EmailAddress::from(user.email);
    let subject = format!("follow-login-{}", Uuid::new_v4().simple());
    f.people
        .role
        .p()
        .link_external_identity(user.id, claim(&subject, &old_email))
        .await
        .unwrap();
    let email = change_address(&f).await;
    edit(&f, None, None).await.unwrap();
    assert!(f.assignment().await.reviewers.is_empty());
    let p = f.people.role.p();
    let stale = p
        .login_external_identity(claim(&subject, &old_email))
        .await
        .unwrap()
        .unwrap();
    assert!(
        stale.email_verified_at.is_none(),
        "an old provider address cannot verify the current one"
    );
    assert!(f.assignment().await.reviewers.is_empty());
    let current = p
        .login_external_identity(claim(&subject, &email))
        .await
        .unwrap()
        .unwrap();
    assert!(current.email_verified_at.is_some());
    assert_eq!(
        f.assignment().await.reviewers.len(),
        1,
        "only a current verified login repairs visibility"
    );
    assert_eq!(state(&f).await["state"], "withdrawn");
}

#[tokio::test]
async fn workflow_reviewer_follow_competing_provider_links_have_one_current_proof_and_assignment() {
    let f = Box::pin(fixture()).await;
    let email = change_address(&f).await;
    let first = format!("first-{}", Uuid::new_v4().simple());
    let second = format!("second-{}", Uuid::new_v4().simple());
    let barrier = tokio::sync::Barrier::new(2);
    let link_first = async {
        barrier.wait().await;
        f.people
            .role
            .p()
            .link_external_identity(f.people.users[0], claim(&first, &email))
            .await
    };
    let link_second = async {
        barrier.wait().await;
        f.people
            .role
            .p()
            .link_external_identity(f.people.users[0], claim(&second, &email))
            .await
    };
    let (a, b) = tokio::join!(link_first, link_second);
    match (a, b) {
        (Ok(()), Err(AppError::Conflict(_))) | (Err(AppError::Conflict(_)), Ok(())) => {}
        outcomes => {
            panic!("expected one committed proof and one identity conflict, got {outcomes:?}")
        }
    }
    assert_eq!(f.assignment().await.reviewers.len(), 1);
    assert_eq!(f.assignment().await.assignment_revision.get(), 3);
    assert_eq!(state(&f).await["state"], "withdrawn");
    let methods: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM user_login_methods WHERE user_id=$1 AND provider='google'",
    )
    .bind(f.people.users[0])
    .fetch_one(f.people.role.p().pool())
    .await
    .unwrap();
    assert_eq!(methods, 1);
}

#[tokio::test]
async fn workflow_reviewer_follow_provider_link_refresh_failure_rolls_back_method_proof_and_assignment()
 {
    let f = Box::pin(fixture()).await;
    let email = change_address(&f).await;
    assert!(f.assignment().await.reviewers.is_empty());
    sqlx::raw_sql("CREATE FUNCTION account_follow_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected proof refresh failure'; END $$; \
        CREATE TRIGGER account_follow_fault BEFORE INSERT ON workflow_reviewer_assignments FOR EACH ROW EXECUTE FUNCTION account_follow_fault()")
        .execute(f.people.role.p().pool()).await.unwrap();
    let subject = format!("rollback-{}", Uuid::new_v4().simple());
    let p = f.people.role.p();
    assert!(
        p.link_external_identity(f.people.users[0], claim(&subject, &email))
            .await
            .is_err()
    );
    let user = UserPersistence::get_by_id(p, f.people.users[0])
        .await
        .unwrap()
        .unwrap();
    assert!(user.email_verified_at.is_none());
    assert!(
        !p.has_login_method(user.id, LoginMethod::Google)
            .await
            .unwrap()
    );
    assert!(f.assignment().await.reviewers.is_empty());
    assert_eq!(state(&f).await["state"], "pending");
}
