use super::*;

#[test]
fn access_mode_public_normalization_preserves_explicit_owner() {
    let department = crate::entities::department::DepartmentId::random();
    let mut write = ChannelWrite {
        name: "Public intake".into(),
        slug: "intake".into(),
        participant_emails: Some(vec![" @PUBLIC ".into()]),
        department_id: Some(department),
        ..Default::default()
    };
    write.normalize().unwrap();
    assert_eq!(write.department_id, Some(department));
    assert!(write.is_public());
}

#[test]
fn access_mode_public_normalization_keeps_an_omitted_owner_omitted() {
    let mut write = ChannelWrite {
        name: "Public intake".into(),
        slug: "intake".into(),
        participant_emails: Some(vec!["@public".into()]),
        ..Default::default()
    };
    write.normalize().unwrap();
    assert_eq!(write.department_id, None);
    assert!(write.is_public());
}

#[test]
fn access_mode_members_normalization_preserves_explicit_owner() {
    let department = crate::entities::department::DepartmentId::random();
    let mut write = ChannelWrite {
        name: "Private intake".into(),
        slug: "intake".into(),
        department_id: Some(department),
        ..Default::default()
    };
    write.normalize().unwrap();
    assert_eq!(write.department_id, Some(department));
    assert!(!write.is_public());
}
