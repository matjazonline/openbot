use super::*;
use crate::entities::{
    access_level::{AccessLevel, LevelTarget},
    company_member::CompanyAccessRole,
};
use chrono::Utc;

struct TeamFixture {
    company: Company,
    member: CompanyMember,
    invite: CompanyInvite,
}
fn fixture() -> TeamFixture {
    let company = Company {
        id: Uuid::new_v4(),
        user_id: Uuid::new_v4(),
        name: "Levels".into(),
        slug: "levels".into(),
        avatar_url: None,
        created_at: Utc::now(),
        channel_defaults: Default::default(),
    };
    let member = CompanyMember {
        id: Uuid::new_v4(),
        company_id: company.id,
        user_id: Uuid::new_v4(),
        username: Some("Colleague".into()),
        email: Some("colleague@example.test".into()),
        avatar_url: None,
        role: CompanyAccessRole::Member,
        access_level: AccessLevel::L3,
        created_at: Utc::now(),
    };
    let invite = CompanyInvite {
        id: Uuid::new_v4(),
        company_id: company.id,
        company_name: Some(company.name.clone()),
        email: "invite@example.test".into(),
        role: CompanyAccessRole::Member,
        access_level: AccessLevel::L3,
        status: "pending".into(),
        created_at: Utc::now(),
    };
    TeamFixture {
        company,
        member,
        invite,
    }
}
#[test]
fn access_level_ui_admin_caps_and_owner_fixed_controls_match_the_domain() {
    let actor = TeamRole::Admin.level_actor(AccessLevel::L4);
    let options = level_options(
        actor,
        LevelTarget::Assignable(AccessLevel::L3),
        AccessLevel::L3,
    );
    assert!(options.contains("value=\"4\""));
    assert!(!options.contains("value=\"5\""));
    assert!(
        level_options(
            actor,
            LevelTarget::Assignable(AccessLevel::L5),
            AccessLevel::PUBLIC
        )
        .is_empty()
    );
    assert!(level_options(actor, LevelTarget::Owner, AccessLevel::MAX).is_empty());
    assert!(
        level_options(
            TeamRole::Member.level_actor(AccessLevel::MAX),
            LevelTarget::Assignable(AccessLevel::PUBLIC),
            AccessLevel::PUBLIC
        )
        .is_empty()
    );
    let fixed = level_control(LevelControl {
        actor: TeamRole::Owner.level_actor(AccessLevel::MAX),
        target: LevelTarget::Owner,
        current: AccessLevel::MAX,
        endpoint: "/unused",
        swap_target: "#team-pane",
    });
    assert!(fixed.contains("Level 9"));
    assert!(!fixed.contains("<select"));
}
#[test]
fn access_level_both_team_pages_give_admins_only_level_controls() {
    let f = fixture();
    let member = member_pane(&MemberPane {
        company: &f.company,
        member: &f.member,
        role: TeamRole::Admin,
        viewer_access_level: AccessLevel::L4,
        viewer_id: Uuid::new_v4(),
        avatar_draft: None,
        error: None,
    });
    let invite = invite_pane(&InvitePane {
        company: &f.company,
        invite: &f.invite,
        role: TeamRole::Admin,
        viewer_access_level: AccessLevel::L4,
        email_draft: None,
        role_draft: None,
        error: None,
    });
    let legacy = company_invites_page(
        &f.company,
        std::slice::from_ref(&f.invite),
        std::slice::from_ref(&f.member),
        TeamRole::Admin.level_actor(AccessLevel::L4),
    );
    for html in [member, invite, legacy] {
        assert!(html.contains("name=\"access_level\""));
        assert!(!html.contains("value=\"5\""));
        for power in [
            "name=\"role\"",
            "name=\"email\"",
            "hx-delete=",
            "Send Invite",
            "/removal",
        ] {
            assert!(!html.contains(power), "unexpected owner control {power}");
        }
        assert!(html.contains("next read"));
        assert!(html.contains("cannot be recalled"));
    }
}
