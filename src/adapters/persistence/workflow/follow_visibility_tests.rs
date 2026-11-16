use super::*;
use crate::application::ports::channel::ChannelUpdate;

#[path = "follow_account_visibility_tests.rs"]
mod accounts;

async fn edit(
    f: &FollowFixture,
    emails: Option<Vec<String>>,
    members_only: Option<bool>,
) -> AppResult<crate::entities::channel::Channel> {
    ChannelPersistence::update(
        f.people.role.p(),
        ChannelUpdate {
            company_id: f.scope.company.as_uuid(),
            channel_id: f.people.role.channel,
            write: ChannelWrite {
                name: "Role intake".into(),
                slug: "role-intake".into(),
                enabled: true,
                participant_emails: emails,
                ownership: members_only.map(|value| ChannelOwnershipWrite {
                    members_only: Some(value),
                    ..Default::default()
                }),
                ..Default::default()
            },
        },
    )
    .await
}

#[tokio::test]
async fn workflow_reviewer_follow_actual_channel_allowlist_edit_refreshes_and_repairs_without_changing_context()
 {
    let f = Box::pin(fixture()).await;
    let proposal = f.proposal().await;
    edit(&f, Some(vec!["other@example.test".into()]), Some(false))
        .await
        .unwrap();
    assert!(
        f.assignment().await.reviewers.is_empty(),
        "the real channel writer must refresh lost visibility"
    );
    assert_eq!(state(&f).await["state"], "pending");
    // Clearing the email allowlist restores team visibility. This generic save carries
    // no department/ownership edit and needs no fabricated verified-email identity.
    edit(&f, None, None).await.unwrap();
    let repaired = f.assignment().await;
    assert_eq!(repaired.reviewers.len(), 1);
    assert_eq!(repaired.assignment_revision.get(), 3);
    assert_eq!(repaired.context_department_id, f.people.role.department);
    assert_eq!(state(&f).await["state"], "withdrawn");
    assert_eq!(f.proposal().await, proposal);
}

async fn ownership(f: &FollowFixture, members_only: bool) {
    let mut tx = f.people.role.p().pool().begin().await.unwrap();
    crate::adapters::persistence::department_desks::set_desk_ownership_on(
        &mut tx,
        f.scope.company,
        f.people.role.channel,
        &ChannelOwnershipWrite {
            members_only: Some(members_only),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn workflow_reviewer_follow_actual_desk_ownership_writer_refreshes_derived_visibility() {
    let f = Box::pin(fixture()).await;
    edit(&f, Some(vec!["other@example.test".into()]), None)
        .await
        .unwrap();
    assert_eq!(
        f.assignment().await.reviewers.len(),
        1,
        "department membership still grants view"
    );
    ownership(&f, false).await;
    assert!(f.assignment().await.reviewers.is_empty());
    assert_eq!(state(&f).await["state"], "pending");
    ownership(&f, true).await;
    assert_eq!(f.assignment().await.reviewers.len(), 1);
    assert_eq!(state(&f).await["state"], "withdrawn");
}

#[tokio::test]
async fn workflow_reviewer_follow_competing_channel_writers_commit_matching_visibility_and_assignment()
 {
    let f = Box::pin(fixture()).await;
    let proposal = f.proposal().await;
    let barrier = tokio::sync::Barrier::new(2);
    let close = async {
        barrier.wait().await;
        edit(&f, Some(vec!["other@example.test".into()]), Some(false))
            .await
            .unwrap();
    };
    let open = async {
        barrier.wait().await;
        edit(&f, None, None).await.unwrap();
    };
    tokio::join!(close, open);
    let visible: bool = sqlx::query_scalar("SELECT workflow_follow_reviewer_eligible($1,$2,$3)")
        .bind(f.scope.company.as_uuid())
        .bind(f.scope.run.as_uuid())
        .bind(f.people.principals[0].as_uuid())
        .fetch_one(f.people.role.p().pool())
        .await
        .unwrap();
    assert_eq!(f.assignment().await.reviewers.len(), usize::from(visible));
    let source = state(&f).await;
    assert_eq!(source["cases"], 1);
    assert_eq!(
        source["state"],
        if visible { "withdrawn" } else { "pending" }
    );
    assert_eq!(f.proposal().await, proposal);
}

async fn channel_state(f: &FollowFixture) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('revision',settings_revision,'mode',access_mode,'members_only',members_only) \
        FROM channels WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company.as_uuid()).bind(f.people.role.channel)
        .fetch_one(f.people.role.p().pool()).await.unwrap()
}

#[tokio::test]
async fn workflow_reviewer_follow_refresh_failure_rolls_back_channel_settings_grants_and_assignments()
 {
    let f = Box::pin(fixture()).await;
    let before = channel_state(&f).await;
    let assignment = f.assignment().await;
    sqlx::raw_sql("CREATE FUNCTION channel_follow_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected refresh failure'; END $$; \
        CREATE TRIGGER channel_follow_fault BEFORE INSERT ON workflow_reviewer_assignments FOR EACH ROW EXECUTE FUNCTION channel_follow_fault()")
        .execute(f.people.role.p().pool()).await.unwrap();
    assert!(
        edit(&f, Some(vec!["other@example.test".into()]), Some(false))
            .await
            .is_err()
    );
    assert_eq!(channel_state(&f).await, before);
    assert_eq!(f.assignment().await, assignment);
    let visible: bool = sqlx::query_scalar("SELECT workflow_follow_reviewer_eligible($1,$2,$3)")
        .bind(f.scope.company.as_uuid())
        .bind(f.scope.run.as_uuid())
        .bind(f.people.principals[0].as_uuid())
        .fetch_one(f.people.role.p().pool())
        .await
        .unwrap();
    assert!(
        visible,
        "the actual derived view grant rolled back with the settings"
    );
    assert_eq!(state(&f).await["cases"], 0);
}
