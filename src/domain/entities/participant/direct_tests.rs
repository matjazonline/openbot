use super::*;

fn principal(kind: PrincipalKind) -> Principal {
    Principal {
        id: PrincipalId::random(),
        company_id: Uuid::new_v4(),
        kind,
        user_id: None,
        agent_id: None,
        display_label: "Direct recipient".into(),
        open_to_public: false,
        settings_revision: 1,
        historical_person_user_id: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn context(membership: CompanyMembership, principal_id: PrincipalId) -> PrincipalAccessContext {
    PrincipalAccessContext {
        principal_id: Some(principal_id),
        membership,
        access_level: crate::entities::access_level::AccessLevel::PUBLIC,
        grant_authority: PrincipalGrantAuthority::Available,
    }
}

#[test]
fn direct_thread_person_is_private_to_self_and_owner() {
    let person = principal(PrincipalKind::Person);
    let other = PrincipalId::random();
    for (membership, id, allowed) in [
        (CompanyMembership::Owner, other, true),
        (CompanyMembership::Member, person.id, true),
        (CompanyMembership::Admin, other, false),
        (CompanyMembership::Member, other, false),
        (CompanyMembership::None, other, false),
        (CompanyMembership::None, person.id, false),
    ] {
        let context = context(membership, id);
        assert_eq!(person.direct_access(context).authorized, allowed);
        assert_eq!(person.direct_viewer_access(context), allowed);
    }
}

#[test]
fn direct_thread_public_agent_admits_mail_without_granting_reads() {
    let mut agent = principal(PrincipalKind::Agent);
    let outsider = context(CompanyMembership::None, PrincipalId::random());
    let teammate = context(CompanyMembership::Member, PrincipalId::random());
    assert!(!agent.direct_access(outsider).authorized);
    assert!(agent.direct_access(teammate).authorized);
    assert!(agent.direct_access(teammate).trusted);
    assert!(agent.direct_viewer_access(teammate));
    agent.open_to_public = true;
    assert!(agent.direct_access(outsider).authorized);
    assert!(!agent.direct_access(outsider).trusted);
    assert!(!agent.direct_viewer_access(outsider));
}

#[test]
fn direct_thread_retired_person_keeps_only_owner_history_access() {
    let mut archived = principal(PrincipalKind::External);
    let owner = context(CompanyMembership::Owner, PrincipalId::random());
    let former = context(CompanyMembership::None, archived.id);
    assert!(!archived.direct_viewer_access(owner));
    archived.historical_person_user_id = Some(Uuid::new_v4());
    assert!(archived.direct_viewer_access(owner));
    assert!(!archived.direct_access(owner).authorized);
    assert!(!archived.direct_viewer_access(former));
    assert!(!archived.direct_viewer_access(context(CompanyMembership::Member, archived.id)));
    for kind in [PrincipalKind::External, PrincipalKind::System] {
        let mut invalid_public = principal(kind);
        invalid_public.open_to_public = true;
        assert!(!invalid_public.direct_access(owner).authorized);
    }
}
