//! Desk forms use required ownership and map the Public checkbox to the canonical marker.
use super::*;

fn form(department_id: Option<&str>, public: bool) -> ChannelForm {
    ChannelForm {
        name: "Inbox".into(),
        description: None,
        slug: Some("inbox".into()),
        alias_slugs: None,
        participant_emails: None,
        confirm_spam_disabled: None,
        enabled: Some("true".into()),
        add_3rd_party: Some("true".into()),
        department_id: department_id.map(str::to_owned),
        public: public.then(|| "true".to_owned()),
    }
}

#[test]
fn access_mode_ui_owner_is_optional_in_a_write_but_cannot_be_cleared() {
    let department = Uuid::new_v4();
    let submitted = SubmittedChannel::new(form(Some(&department.to_string()), false));
    assert_eq!(
        submitted.write().unwrap().department_id,
        Some(DepartmentId::new(department))
    );
    assert_eq!(submitted.draft().department_id, Some(department));
    assert_eq!(
        SubmittedChannel::new(form(None, false))
            .write()
            .unwrap()
            .department_id,
        None
    );
    for invalid in ["", " ", "sales"] {
        assert!(matches!(
            SubmittedChannel::new(form(Some(invalid), false)).write(),
            Err(AppError::BadRequest(_))
        ));
    }
}

#[test]
fn access_mode_ui_public_checkbox_preserves_named_addresses_and_can_close_intake() {
    let mut opened = form(None, true);
    opened.participant_emails = Some("person@example.test".into());
    let opened = SubmittedChannel::new(opened);
    assert!(opened.write().unwrap().is_public());
    assert_eq!(
        opened.write().unwrap().participant_emails.unwrap(),
        ["person@example.test", "@public"]
    );
    let mut closed = form(None, false);
    closed.participant_emails = Some("person@example.test, @public".into());
    let closed = SubmittedChannel::new(closed);
    assert!(!closed.write().unwrap().is_public());
    assert_eq!(
        closed.write().unwrap().participant_emails.unwrap(),
        ["person@example.test"]
    );
}
