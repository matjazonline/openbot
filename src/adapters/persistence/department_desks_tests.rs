//! Desks owned by departments against PostgreSQL (01-departments p02): `department_desk_ownership_`
//! for assigning and detaching a desk, `department_desk_members_only_` for who a members-only
//! desk admits as membership, roles and the tree change, and `department_desk_competing_` for the
//! writers that race them.
use super::*;
use crate::application::ports::channel::{ChannelOwnershipWrite, ChannelPersistence, ChannelWrite};
use crate::entities::access_level::AccessLevel;
use crate::entities::channel::{Channel, ChannelAccessMode};
use crate::entities::company_member::CompanyMembership;
use crate::entities::participant::{
    PrincipalAccessContext, PrincipalCapability, PrincipalGrantAuthority,
};
use crate::use_cases::agent::SpamScanning;
use crate::use_cases::channel::ChannelUseCases;

fn channels(f: &Departments) -> ChannelUseCases {
    ChannelUseCases::new(
        Arc::new(f.persistence.clone()),
        Arc::new(f.persistence.clone()),
        Arc::new(f.persistence.clone()),
        SpamScanning::Available,
    )
}

fn owned(department: DepartmentId, members_only: bool) -> Option<ChannelOwnershipWrite> {
    Some(ChannelOwnershipWrite {
        department_id: Some(Some(department)),
        members_only: Some(members_only),
    })
}

fn desk_write(slug: &str, ownership: Option<ChannelOwnershipWrite>) -> ChannelWrite {
    ChannelWrite {
        name: slug.into(),
        slug: slug.into(),
        enabled: true,
        ownership,
        ..ChannelWrite::default()
    }
}

async fn desk(f: &Departments, slug: &str, ownership: Option<ChannelOwnershipWrite>) -> Channel {
    channels(f)
        .create_channel(f.owner, f.company, desk_write(slug, ownership), false)
        .await
        .expect("the owner creates a desk")
}

/// Save the desk's current settings with `ownership` and `emails`, as a form or the JSON API does.
async fn save(
    f: &Departments,
    desk: &Channel,
    ownership: Option<ChannelOwnershipWrite>,
    emails: Option<Vec<&str>>,
) -> AppResult<Channel> {
    channels(f)
        .update_channel(
            f.owner,
            f.company,
            desk.id,
            ChannelWrite {
                participant_emails: emails
                    .map(|emails| emails.into_iter().map(String::from).collect()),
                ..desk_write(desk.slug.as_str(), ownership)
            },
            false,
        )
        .await
}

async fn reload(f: &Departments, id: Uuid) -> Channel {
    ChannelPersistence::get_by_id(&f.persistence, id)
        .await
        .unwrap()
        .expect("the desk exists")
}

/// The desk's department-derived grants, sorted.
fn derived(desk: &Channel) -> Vec<(PrincipalId, PrincipalCapability)> {
    let mut grants: Vec<_> = desk
        .principal_grants
        .iter()
        .filter(|grant| grant.provenance == GrantProvenance::DepartmentMembership)
        .map(|grant| (grant.principal_id, grant.capability))
        .collect();
    grants.sort_by_key(|(principal, capability)| (*principal, capability.as_str()));
    grants
}

fn both(principals: &[PrincipalId]) -> Vec<(PrincipalId, PrincipalCapability)> {
    let mut grants: Vec<_> = principals
        .iter()
        .flat_map(|principal| {
            [
                (*principal, PrincipalCapability::Participate),
                (*principal, PrincipalCapability::View),
            ]
        })
        .collect();
    grants.sort_by_key(|(principal, capability)| (*principal, capability.as_str()));
    grants
}

fn context(principal: PrincipalId, membership: CompanyMembership) -> PrincipalAccessContext {
    PrincipalAccessContext {
        principal_id: Some(principal),
        membership,
        access_level: AccessLevel::PUBLIC,
        grant_authority: PrincipalGrantAuthority::Available,
    }
}

/// Channel policy and the SQL notification rule, both reading the stored grants.
async fn admits(
    f: &Departments,
    desk: Uuid,
    principal: PrincipalId,
    membership: CompanyMembership,
) -> (bool, bool, bool) {
    let channel = reload(f, desk).await;
    let can_view: bool = sqlx::query_scalar("SELECT notification_principal_can_view($1, $2, $3)")
        .bind(f.company)
        .bind(desk)
        .bind(principal.as_uuid())
        .fetch_one(f.pool())
        .await
        .unwrap();
    (
        channel
            .participant_access(context(principal, membership))
            .authorized,
        channel.viewer_access(context(principal, membership)),
        can_view,
    )
}

/// A teammate whose person principal owns their account email identity, so an allowlist entry
/// for that address resolves to them.
async fn teammate_with_address(f: &Departments, label: &str) -> (Uuid, PrincipalId, String) {
    let user = user(f.pool(), label).await;
    let email: String = sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(f.pool())
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO company_members (id, company_id, user_id, role, access_level) \
         VALUES ($1, $2, $3, 'member', 0)",
    )
    .bind(Uuid::new_v4())
    .bind(f.company)
    .bind(user)
    .execute(f.pool())
    .await
    .unwrap();
    let mut connection = f.pool().acquire().await.unwrap();
    let principal = crate::adapters::persistence::participant::create_person_principal_on(
        &mut connection,
        f.company,
        user,
        label,
        &email,
    )
    .await
    .unwrap();
    (user, principal, email)
}

async fn add(f: &Departments, department: DepartmentId, principal: PrincipalId) {
    f.use_cases
        .add_member(f.owner, f.company, department, principal)
        .await
        .unwrap();
}

async fn lead(f: &Departments, department: DepartmentId, principal: PrincipalId) {
    f.use_cases
        .assign_role(
            f.owner,
            f.company,
            department,
            DepartmentRole::Lead,
            principal,
        )
        .await
        .unwrap();
}

// --- department_desk_ownership_ ---------------------------------------------------------------

#[tokio::test]
async fn department_desk_ownership_refuses_another_companys_department() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let foreign = DepartmentId::new(
        f.insert_raw(f.foreign_company, "abroad", None)
            .await
            .unwrap(),
    );
    let plain = desk(&f, "plain", None).await;
    let refused = save(&f, &plain, owned(foreign, false), None).await;
    assert!(matches!(refused, Err(AppError::NotFound(_))), "{refused:?}");
    let created = channels(&f)
        .create_channel(
            f.owner,
            f.company,
            desk_write("stray", owned(foreign, false)),
            false,
        )
        .await;
    assert!(matches!(created, Err(AppError::NotFound(_))), "{created:?}");
    assert_eq!(reload(&f, plain.id).await.department_id, None);
}

#[tokio::test]
async fn department_desk_ownership_blocks_deleting_its_department_until_detached() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let sales = f.create("Sales", None).await;
    let plain = desk(&f, "pipeline", None).await;
    let assigned = save(&f, &plain, owned(sales.id, false), None)
        .await
        .unwrap();
    assert_eq!(assigned.department_id, Some(sales.id));
    assert!(!assigned.members_only);
    assert!(
        assigned.settings_revision > plain.settings_revision,
        "an ownership edit is a settings edit"
    );

    // A save from a form that knows nothing of departments keeps the ownership.
    let kept = save(&f, &assigned, None, None).await.unwrap();
    assert_eq!(
        (kept.department_id, kept.members_only),
        (Some(sales.id), false)
    );

    let refused = f
        .use_cases
        .delete_department(f.owner, f.company, sales.id)
        .await;
    assert!(matches!(refused, Err(AppError::Conflict(_))), "{refused:?}");

    let detach = Some(ChannelOwnershipWrite {
        department_id: Some(None),
        members_only: None,
    });
    let detached = save(&f, &kept, detach, None).await.unwrap();
    assert_eq!(detached.department_id, None);
    assert!(detached.settings_revision > kept.settings_revision);
    f.use_cases
        .delete_department(f.owner, f.company, sales.id)
        .await
        .expect("a department with no desks deletes");
}

#[tokio::test]
async fn department_desk_ownership_company_teardown_takes_owned_desks_with_it() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let ops = f.create("Ops", None).await;
    add(&f, ops.id, f.member_principal).await;
    desk(&f, "ops-desk", owned(ops.id, true)).await;
    let refused = sqlx::query("DELETE FROM departments WHERE id = $1")
        .bind(ops.id.as_uuid())
        .execute(f.pool())
        .await;
    assert_eq!(refused_by(refused), "channels_department_fk");
    let mut tx = f.pool().begin().await.unwrap();
    sqlx::query("DELETE FROM companies WHERE id = $1")
        .bind(f.company)
        .execute(&mut *tx)
        .await
        .expect("a company teardown removes departments and their desks together");
    tx.rollback().await.unwrap();
}

/// DEP-P02-D4 invariant (G1-F3): only members-only desks hold derived grants, and moving a desk
/// replaces the old department's with the new one's.
#[tokio::test]
async fn department_desk_ownership_moving_or_opening_a_desk_rederives_its_grants() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let first = f.create("First", None).await;
    let second = f.create("Second", None).await;
    add(&f, first.id, f.member_principal).await;
    add(&f, second.id, f.agent_principal).await;
    let shared = desk(&f, "shared", owned(first.id, true)).await;
    assert_eq!(shared.access_mode, ChannelAccessMode::Allowlist);
    assert_eq!(derived(&shared), both(&[f.member_principal]));

    let moved = save(&f, &shared, owned(second.id, true), None)
        .await
        .unwrap();
    assert_eq!(derived(&moved), both(&[f.agent_principal]));

    let opened = save(&f, &moved, owned(second.id, false), None)
        .await
        .unwrap();
    assert_eq!(derived(&opened), Vec::new());
    assert_eq!(
        opened.access_mode,
        ChannelAccessMode::Team,
        "the email list decides again"
    );
    // A later membership change does not bring them back on a desk that is not members-only.
    add(&f, second.id, f.member_principal).await;
    assert_eq!(derived(&reload(&f, shared.id).await), Vec::new());
}

// --- department_desk_members_only_ ------------------------------------------------------------

#[tokio::test]
async fn department_desk_members_only_admits_members_and_the_owner_only() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let finance = f.create("Finance", None).await;
    add(&f, finance.id, f.member_principal).await;
    add(&f, finance.id, f.agent_principal).await;
    let (_, outsider) = teammate(f.pool(), f.company, "member").await;
    let books = desk(&f, "books", owned(finance.id, true)).await;
    assert_eq!(
        derived(&books),
        both(&[f.member_principal, f.agent_principal]),
        "agent members get derived grants too"
    );
    assert_eq!(
        admits(&f, books.id, f.member_principal, CompanyMembership::Member).await,
        (true, true, true)
    );
    assert_eq!(
        admits(&f, books.id, outsider, CompanyMembership::Member).await,
        (false, false, false),
        "a teammate outside the department"
    );
    assert_eq!(
        admits(&f, books.id, f.owner_principal, CompanyMembership::Owner).await,
        (true, true, true),
        "the owner exemption"
    );
}

#[tokio::test]
async fn department_desk_members_only_removal_revokes_access_and_notifications_in_one_commit() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let legal = f.create("Legal", None).await;
    add(&f, legal.id, f.member_principal).await;
    let cases = desk(&f, "cases", owned(legal.id, true)).await;
    let event: Uuid = sqlx::query_scalar(
        "INSERT INTO notification_events (company_id, source_kind, source_id, action_kind, \
           source_generation) VALUES ($1, 'delivery', $2, 'delivery_failure', 1) RETURNING id",
    )
    .bind(f.company)
    .bind(Uuid::new_v4())
    .fetch_one(f.pool())
    .await
    .unwrap();
    let notification: Uuid = sqlx::query_scalar(
        "INSERT INTO notifications (company_id, recipient_user_id, recipient_principal_id, \
           event_id, source_kind, source_id, action_kind, source_generation, channel_id) \
         SELECT company_id, $2, $3, id, source_kind, source_id, action_kind, source_generation, $4 \
           FROM notification_events WHERE id = $1 RETURNING id",
    )
    .bind(event)
    .bind(f.member)
    .bind(f.member_principal.as_uuid())
    .bind(cases.id)
    .fetch_one(f.pool())
    .await
    .unwrap();
    let before = reload(&f, cases.id).await.settings_revision;

    f.use_cases
        .remove_member(f.owner, f.company, legal.id, f.member_principal)
        .await
        .unwrap();
    let after = reload(&f, cases.id).await;
    assert_eq!(derived(&after), Vec::new());
    assert!(after.settings_revision > before);
    assert_eq!(
        admits(&f, cases.id, f.member_principal, CompanyMembership::Member).await,
        (false, false, false)
    );
    let state: String = sqlx::query_scalar("SELECT state FROM notifications WHERE id = $1")
        .bind(notification)
        .fetch_one(f.pool())
        .await
        .unwrap();
    assert_eq!(state, "withdrawn");
}

/// One step both ways (01DEP-C002): the parent's lead and each child's lead, nobody further.
#[tokio::test]
async fn department_desk_members_only_reaches_one_step_up_and_down() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let grand = f.create("Grand", None).await;
    let parent = f.create("Parent", Some(grand.id)).await;
    let middle = f.create("Middle", Some(parent.id)).await;
    let child = f.create("Child", Some(middle.id)).await;
    let grandchild = f.create("Grandchild", Some(child.id)).await;
    let mut leads = Vec::new();
    for department in [&grand, &parent, &child, &grandchild] {
        let (_, principal) = teammate(f.pool(), f.company, "member").await;
        lead(&f, department.id, principal).await;
        leads.push(principal);
    }
    let [grand_lead, parent_lead, child_lead, grandchild_lead] = leads[..] else {
        unreachable!()
    };
    let room = desk(&f, "room", owned(middle.id, true)).await;
    assert_eq!(derived(&room), both(&[parent_lead, child_lead]));
    for (principal, expected) in [
        (parent_lead, true),
        (child_lead, true),
        (grand_lead, false),
        (grandchild_lead, false),
    ] {
        assert_eq!(
            admits(&f, room.id, principal, CompanyMembership::Member)
                .await
                .0,
            expected
        );
    }

    // A new parent lead and a re-parent both follow.
    let (_, new_parent_lead) = teammate(f.pool(), f.company, "member").await;
    lead(&f, parent.id, new_parent_lead).await;
    assert_eq!(
        derived(&reload(&f, room.id).await),
        both(&[new_parent_lead, child_lead])
    );
    f.reparent(middle.id, Some(grand.id)).await.unwrap();
    assert_eq!(
        derived(&reload(&f, room.id).await),
        both(&[grand_lead, child_lead])
    );
    // The middle department's own lead reaches the room, its parent's and its child's desks.
    let (_, middle_lead) = teammate(f.pool(), f.company, "member").await;
    let up = desk(&f, "grand-room", owned(grand.id, true)).await;
    let down = desk(&f, "child-room", owned(child.id, true)).await;
    lead(&f, middle.id, middle_lead).await;
    for room in [room.id, up.id, down.id] {
        assert!(
            derived(&reload(&f, room).await)
                .contains(&(middle_lead, PrincipalCapability::Participate)),
            "{room}"
        );
    }
    f.use_cases
        .clear_role(
            f.owner,
            f.company,
            middle.id,
            DepartmentRole::Lead,
            middle_lead,
        )
        .await
        .unwrap();
    for room in [room.id, up.id, down.id] {
        assert!(
            !derived(&reload(&f, room).await).contains(&(middle_lead, PrincipalCapability::View)),
            "{room}"
        );
    }
}

/// G1-F1: a department grant and an allowlist entry for the same person are separate rows, so
/// neither removes the access the other gave, in either order.
#[tokio::test]
async fn department_desk_members_only_keeps_an_allowlist_entry_and_a_membership_apart() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let audit = f.create("Audit", None).await;
    let room = desk(&f, "audit-room", owned(audit.id, true)).await;

    // Allowlisted first, then a member; a save dropping the email keeps the member's access.
    let (_, first, first_email) = teammate_with_address(&f, "first").await;
    let listed = save(&f, &room, None, Some(vec![first_email.as_str()]))
        .await
        .unwrap();
    assert!(
        listed
            .participant_emails
            .as_ref()
            .unwrap()
            .iter()
            .any(|email| email.as_str() == first_email)
    );
    add(&f, audit.id, first).await;
    let unlisted = save(&f, &reload(&f, room.id).await, None, Some(vec![]))
        .await
        .unwrap();
    assert_eq!(unlisted.access_mode, ChannelAccessMode::Allowlist);
    assert_eq!(
        unlisted.participant_emails,
        Some(vec![]),
        "a derived holder is not shown as an allowlist email (02B.3)"
    );
    assert_eq!(derived(&unlisted), both(&[first]));
    assert!(
        admits(&f, room.id, first, CompanyMembership::Member)
            .await
            .0
    );

    // A member first, then allowlisted; leaving keeps the allowlisted access and its email.
    let (_, second, second_email) = teammate_with_address(&f, "second").await;
    add(&f, audit.id, second).await;
    save(
        &f,
        &reload(&f, room.id).await,
        None,
        Some(vec![second_email.as_str()]),
    )
    .await
    .unwrap();
    f.use_cases
        .remove_member(f.owner, f.company, audit.id, second)
        .await
        .unwrap();
    let after = reload(&f, room.id).await;
    assert!(
        after
            .participant_emails
            .as_ref()
            .unwrap()
            .iter()
            .any(|email| email.as_str() == second_email)
    );
    assert!(
        !derived(&after)
            .iter()
            .any(|(principal, _)| *principal == second)
    );
    assert!(
        admits(&f, room.id, second, CompanyMembership::Member)
            .await
            .0
    );
}

/// DEP-P02-D2 (G1-F2): every save applies the members-only rule to the desk's ownership.
#[tokio::test]
async fn department_desk_members_only_form_saves_keep_allowlist_and_refuse_public() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let hr = f.create("HR", None).await;
    let people = desk(&f, "people", owned(hr.id, true)).await;
    let saved = save(&f, &people, None, Some(vec![])).await.unwrap();
    assert_eq!(
        (saved.access_mode, saved.members_only),
        (ChannelAccessMode::Allowlist, true)
    );
    let public = save(&f, &saved, None, Some(vec!["@public"])).await;
    assert!(matches!(public, Err(AppError::BadRequest(_))), "{public:?}");
    let unowned = save(
        &f,
        &saved,
        Some(ChannelOwnershipWrite {
            department_id: Some(None),
            members_only: None,
        }),
        None,
    )
    .await;
    assert!(
        matches!(unowned, Err(AppError::BadRequest(_))),
        "{unowned:?}"
    );
    assert_eq!(
        reload(&f, people.id).await.department_id,
        Some(hr.id),
        "nothing changed"
    );
}

/// DEP-P02-D5: a teammate removed from the company loses their derived grants with their
/// department places; an allowlist entry on the same principal stays.
#[tokio::test]
async fn department_desk_members_only_removing_a_teammate_drops_only_derived_grants() {
    use crate::use_cases::company_invite::CompanyInvitePersistence;
    let Some(f) = Departments::new().await else {
        return;
    };
    let support = f.create("Support", None).await;
    let room = desk(&f, "support-room", owned(support.id, true)).await;
    let (user, leaving, email) = teammate_with_address(&f, "leaving").await;
    add(&f, support.id, leaving).await;
    save(
        &f,
        &reload(&f, room.id).await,
        None,
        Some(vec![email.as_str()]),
    )
    .await
    .unwrap();
    let before = reload(&f, room.id).await.settings_revision;
    CompanyInvitePersistence::remove_member(&f.persistence, f.company, user)
        .await
        .unwrap();
    let after = reload(&f, room.id).await;
    assert!(
        !derived(&after)
            .iter()
            .any(|(principal, _)| *principal == leaving)
    );
    assert!(
        after.settings_revision > before,
        "the trigger moved the desk's settings on"
    );
    assert!(
        after
            .principal_grants
            .iter()
            .any(|grant| grant.principal_id == leaving
                && grant.provenance == GrantProvenance::ConfiguredAllowlist),
        "the allowlist entry is not the department's to remove"
    );
}

#[tokio::test]
async fn department_desk_members_only_set_desk_ownership_on_takes_the_desk_edit_path() {
    use crate::adapters::persistence::department_desks::set_desk_ownership_on;
    let Some(f) = Departments::new().await else {
        return;
    };
    let crew = f.create("Crew", None).await;
    add(&f, crew.id, f.member_principal).await;
    let plain = desk(&f, "crew-room", None).await;
    let mut tx = f.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id = $1 FOR NO KEY UPDATE")
        .bind(f.company)
        .execute(&mut *tx)
        .await
        .unwrap();
    set_desk_ownership_on(
        &mut tx,
        CompanyId::new(f.company),
        plain.id,
        &ChannelOwnershipWrite {
            department_id: Some(Some(crew.id)),
            members_only: Some(true),
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let after = reload(&f, plain.id).await;
    assert_eq!(
        (after.department_id, after.members_only, after.access_mode),
        (Some(crew.id), true, ChannelAccessMode::Allowlist)
    );
    assert_eq!(derived(&after), both(&[f.member_principal]));
}

// --- department_desk_competing_ ---------------------------------------------------------------

/// A membership change and a desk's ownership edit, lined up behind the company lock, leave the
/// desk's grants matching the final membership whichever commits first.
#[tokio::test]
async fn department_desk_competing_membership_and_ownership_edit_agree() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = Arc::new(Departments::on(database.pool.clone()).await);
    let team = f.create("Team", None).await;
    let room = desk(&f, "team-room", owned(team.id, false)).await;
    let blocker = super::competing::hold_company(&f).await;
    let joining = tokio::spawn({
        let f = f.clone();
        async move {
            f.use_cases
                .add_member(f.owner, f.company, team.id, f.member_principal)
                .await
        }
    });
    let closing = tokio::spawn({
        let f = f.clone();
        let room = room.clone();
        async move { save(&f, &room, owned(team.id, true), None).await.map(drop) }
    });
    wait_until_backends_are_blocked(f.pool(), 2).await;
    blocker.commit().await.unwrap();
    joining.await.unwrap().expect("the member joins");
    closing
        .await
        .unwrap()
        .expect("the desk becomes members-only");
    assert_eq!(
        derived(&reload(&f, room.id).await),
        both(&[f.member_principal])
    );
}

/// A re-derivation waiting on a principal being demoted reads the demotion's outcome and leaves
/// the demoted principal out.
#[tokio::test]
async fn department_desk_competing_rederivation_behind_a_demotion_leaves_it_out() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = Arc::new(Departments::on(database.pool.clone()).await);
    let unit = f.create("Unit", None).await;
    add(&f, unit.id, f.member_principal).await;
    let room = desk(&f, "unit-room", owned(unit.id, true)).await;
    assert_eq!(derived(&room), both(&[f.member_principal]));
    let mut demotion = f.pool().begin().await.unwrap();
    sqlx::query("UPDATE principals SET kind = 'external', user_id = NULL WHERE id = $1")
        .bind(f.member_principal.as_uuid())
        .execute(&mut *demotion)
        .await
        .unwrap();
    let joining = tokio::spawn({
        let f = f.clone();
        async move {
            f.use_cases
                .add_member(f.owner, f.company, unit.id, f.agent_principal)
                .await
        }
    });
    wait_until_backends_are_blocked(f.pool(), 1).await;
    demotion.commit().await.unwrap();
    joining.await.unwrap().expect("the agent joins");
    assert_eq!(
        derived(&reload(&f, room.id).await),
        both(&[f.agent_principal])
    );
}

/// 01DEP-P02-B1-F1: a removal paused after deleting its row, and a demotion of that principal
/// started meanwhile, both commit: the removal held the principal before touching its rows, so
/// the demotion waits for it instead of forming a cycle with the re-derivation.
#[tokio::test]
async fn department_desk_competing_removal_and_demotion_of_one_principal_both_commit() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = Arc::new(Departments::on(database.pool.clone()).await);
    let unit = f.create("Pair", None).await;
    add(&f, unit.id, f.member_principal).await;
    lead(&f, unit.id, f.member_principal).await;
    let room = desk(&f, "pair-room", owned(unit.id, true)).await;
    assert_eq!(derived(&room), both(&[f.member_principal]));
    sqlx::query(&format!(
        r#"CREATE FUNCTION pause_department_removal() RETURNS trigger LANGUAGE plpgsql AS $$
           BEGIN IF OLD.principal_id = '{}'::uuid THEN PERFORM pg_advisory_xact_lock(61018); END IF;
           RETURN OLD; END $$"#,
        f.member_principal
    ))
    .execute(f.pool())
    .await
    .unwrap();
    sqlx::query(
        "CREATE TRIGGER zz_pause_department_removal AFTER DELETE ON department_members \
         FOR EACH ROW EXECUTE FUNCTION pause_department_removal()",
    )
    .execute(f.pool())
    .await
    .unwrap();
    let mut gate = f.pool().begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(61018)")
        .execute(&mut *gate)
        .await
        .unwrap();
    let removal = tokio::spawn({
        let f = f.clone();
        async move {
            f.use_cases
                .remove_member(f.owner, f.company, unit.id, f.member_principal)
                .await
        }
    });
    wait_until_backends_are_blocked(f.pool(), 1).await;
    let demotion = tokio::spawn({
        let pool = f.pool().clone();
        let principal = f.member_principal;
        async move {
            sqlx::query("UPDATE principals SET kind = 'external', user_id = NULL WHERE id = $1")
                .bind(principal.as_uuid())
                .execute(&pool)
                .await
        }
    });
    wait_until_backends_are_blocked(f.pool(), 2).await;
    gate.commit().await.unwrap();
    removal.await.unwrap().expect("the removal commits");
    demotion
        .await
        .unwrap()
        .expect("the demotion commits, no deadlock");
    let after = reload(&f, room.id).await;
    assert_eq!(
        derived(&after),
        Vec::new(),
        "still lead until demoted, then nothing"
    );
    assert!(f.roles(unit.id).await.is_empty());
}
