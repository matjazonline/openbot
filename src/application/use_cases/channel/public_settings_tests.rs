use super::*;

#[test]
fn public_desk_private_agent_public_selection_clears_only_the_members_only_flag() {
    let department = crate::entities::department::DepartmentId::new(Uuid::new_v4());
    let mut write = ChannelWrite {
        name: "Public intake".into(),
        slug: "intake".into(),
        participant_emails: Some(vec![" @PUBLIC ".into()]),
        ownership: Some(crate::application::ports::channel::ChannelOwnershipWrite {
            department_id: Some(Some(department)),
            members_only: Some(true),
        }),
        ..ChannelWrite::default()
    };
    write.normalize().unwrap();
    let ownership = write.ownership.unwrap();
    assert_eq!(ownership.department_id, Some(Some(department)));
    assert_eq!(ownership.members_only, Some(false));
    assert!(write.is_public());
}

#[test]
fn public_desk_private_agent_public_selection_clears_a_hidden_members_only_setting() {
    let mut write = ChannelWrite {
        name: "Public intake".into(),
        slug: "intake".into(),
        participant_emails: Some(vec!["@public".into()]),
        ..ChannelWrite::default()
    };
    write.normalize().unwrap();
    let ownership = write.ownership.unwrap();
    assert_eq!(
        ownership.department_id, None,
        "keep the existing department"
    );
    assert_eq!(ownership.members_only, Some(false));
}

#[test]
fn public_desk_private_agent_private_selection_preserves_members_only() {
    let mut write = ChannelWrite {
        name: "Private intake".into(),
        slug: "intake".into(),
        ownership: Some(crate::application::ports::channel::ChannelOwnershipWrite {
            department_id: None,
            members_only: Some(true),
        }),
        ..ChannelWrite::default()
    };
    write.normalize().unwrap();
    assert_eq!(write.ownership.unwrap().members_only, Some(true));
}
