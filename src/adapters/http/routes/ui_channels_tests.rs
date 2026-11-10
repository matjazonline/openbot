//! The `/ui` desk form's owner fields (01-departments 04B.1): what ownership change a submission
//! asks for. Persisting it is `ChannelWrite::ownership`, covered by the p02 desk tests.
//! `department_ui_desk_form_`.
use super::*;

fn form(department_id: Option<&str>, members_only: bool) -> ChannelForm {
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
        members_only: members_only.then(|| "true".to_owned()),
    }
}

fn stored(department_id: Option<Uuid>, members_only: bool) -> Channel {
    Channel {
        id: Uuid::new_v4(),
        company_id: Uuid::new_v4(),
        name: "Inbox".into(),
        description: None,
        slug: "inbox".into(),
        alias_slugs: Vec::new(),
        participant_emails: None,
        access_mode: crate::entities::channel::ChannelAccessMode::Team,
        principal_grants: Vec::new(),
        enabled: true,
        add_3rd_party: true,
        created_by: crate::entities::creation::CreationProvenance::system(),
        created_at: chrono::Utc::now(),
        settings_revision: 0,
        department_id: department_id.map(DepartmentId::new),
        members_only,
    }
}

fn ownership(
    form: ChannelForm,
    desk: Option<&Channel>,
) -> AppResult<Option<ChannelOwnershipWrite>> {
    SubmittedChannel::new(form)
        .write(desk)
        .map(|write| write.ownership)
}

#[test]
fn department_ui_desk_form_asks_only_for_the_ownership_it_changes() {
    let sales = Uuid::new_v4();
    let owned = stored(Some(sales), true);
    // Unchanged: no ownership write, so a plain save takes no department lock.
    assert_eq!(
        ownership(form(Some(&sales.to_string()), true), Some(&owned)).unwrap(),
        None
    );
    // A form without the owner select (the classic page) never touches ownership.
    assert_eq!(ownership(form(None, false), Some(&owned)).unwrap(), None);
    // Clearing the select detaches the desk and drops members-only with it.
    assert_eq!(
        ownership(form(Some(""), true), Some(&owned)).unwrap(),
        Some(ChannelOwnershipWrite {
            department_id: Some(None),
            members_only: Some(false),
        })
    );
    // Assigning a department to an unowned desk, members-only ticked.
    let plain = stored(None, false);
    assert_eq!(
        ownership(form(Some(&sales.to_string()), true), Some(&plain)).unwrap(),
        Some(ChannelOwnershipWrite {
            department_id: Some(Some(DepartmentId::new(sales))),
            members_only: Some(true),
        })
    );
    // Members-only without a department is ignored, so nothing changes.
    assert_eq!(ownership(form(Some(""), true), Some(&plain)).unwrap(), None);
    // A new desk asks for ownership only when a department is chosen.
    assert_eq!(ownership(form(Some(""), false), None).unwrap(), None);
    assert!(
        ownership(form(Some(&sales.to_string()), false), None)
            .unwrap()
            .is_some()
    );
    // A value that is not an id is refused, not ignored.
    assert!(matches!(
        ownership(form(Some("sales"), false), Some(&plain)),
        Err(AppError::BadRequest(_))
    ));
    // The draft a refused save re-renders keeps the choice.
    let submitted = SubmittedChannel::new(form(Some(&sales.to_string()), true));
    let draft = submitted.draft();
    assert_eq!(draft.department_id, Some(sales));
    assert!(draft.members_only);
}
