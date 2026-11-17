//! Submit the controls emitted by the live classic renderers through the real handlers.
use super::super::authorization_test_support::Fixture;
use super::*;
use crate::application::ports::channel::ChannelPersistence;
use crate::application::ports::department::DepartmentWrite;
use crate::entities::channel::ChannelAccessMode;
use html5ever::{parse_document, tendril::TendrilSink};
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use std::collections::BTreeMap;

fn descendants(root: &Handle) -> Vec<Handle> {
    let mut pending = vec![root.clone()];
    let mut nodes = Vec::new();
    while let Some(node) = pending.pop() {
        pending.extend(node.children.borrow().iter().rev().cloned());
        nodes.push(node);
    }
    nodes
}

fn attribute(node: &Handle, key: &str) -> Option<String> {
    let NodeData::Element { attrs, .. } = &node.data else {
        return None;
    };
    attrs
        .borrow()
        .iter()
        .find(|attr| attr.name.local.as_ref() == key)
        .map(|attr| attr.value.to_string())
}

fn element(node: &Handle, tag: &str) -> bool {
    matches!(&node.data, NodeData::Element { name, .. } if name.local.as_ref() == tag)
}

/// These desk forms contain named inputs and selects, all parsed with the real HTML parser.
/// Browser tests separately verify native FormData and the shipped checkbox event handlers.
// The caller retains the DOM owner while traversing child handles.
fn form_nodes(document: &RcDom, form_id: &str) -> Vec<Handle> {
    let form = descendants(&document.document)
        .into_iter()
        .find(|node| element(node, "form") && attribute(node, "id").as_deref() == Some(form_id))
        .expect("the route rendered its form");
    descendants(&form)
}

fn check(html: &str, form_id: &str, data: &mut BTreeMap<String, String>, name: &str) {
    let document = parse_document(RcDom::default(), Default::default()).one(html);
    let checkbox = form_nodes(&document, form_id)
        .into_iter()
        .find(|node| element(node, "input") && attribute(node, "name").as_deref() == Some(name))
        .expect("the real renderer must supply the checkbox being selected");
    assert_eq!(attribute(&checkbox, "type").as_deref(), Some("checkbox"));
    data.insert(name.to_owned(), attribute(&checkbox, "value").unwrap());
}

fn controls(html: &str, form_id: &str) -> BTreeMap<String, String> {
    let document = parse_document(RcDom::default(), Default::default()).one(html);
    let mut data = BTreeMap::new();
    for node in form_nodes(&document, form_id) {
        let Some(name) = attribute(&node, "name") else {
            continue;
        };
        if attribute(&node, "disabled").is_some() {
            continue;
        }
        if element(&node, "input") {
            if attribute(&node, "type").as_deref() == Some("checkbox")
                && attribute(&node, "checked").is_none()
            {
                continue;
            }
            data.insert(name, attribute(&node, "value").unwrap_or_default());
        } else if element(&node, "select") {
            assert!(attribute(&node, "required").is_some());
            let selected = descendants(&node)
                .into_iter()
                .find(|option| element(option, "option") && attribute(option, "selected").is_some())
                .unwrap_or_else(|| {
                    panic!(
                        "select {name} has no selected owner; options={:?}",
                        descendants(&node)
                            .into_iter()
                            .filter(|option| element(option, "option"))
                            .map(|option| (
                                attribute(&option, "value"),
                                attribute(&option, "selected")
                            ))
                            .collect::<Vec<_>>()
                    )
                });
            data.insert(name, attribute(&selected, "value").unwrap());
        }
    }
    data
}

fn submitted(data: &BTreeMap<String, String>) -> Form<ChannelForm> {
    Form(serde_urlencoded::from_str(&serde_urlencoded::to_string(data).unwrap()).unwrap())
}

async fn create_page(f: &Fixture) -> String {
    list_channels_page(
        State(f.companies.clone()),
        State(f.channels.clone()),
        State(f.departments()),
        State(f.config.clone()),
        Fixture::auth(&f.owner),
        Path(f.company.id),
        Query(ChannelsPageQuery {
            new: Some("1".into()),
            edit: None,
        }),
    )
    .await
    .unwrap()
    .0
}

async fn edit_page(f: &Fixture, desk: Uuid) -> String {
    edit_channel_form(
        State(f.companies.clone()),
        State(f.channels.clone()),
        State(f.config.clone()),
        State(f.departments()),
        Fixture::auth(&f.owner),
        Path((f.company.id, desk)),
    )
    .await
    .unwrap()
    .0
}

async fn save(f: &Fixture, desk: Uuid, data: &BTreeMap<String, String>) -> String {
    update_channel_handler(
        State(f.companies.clone()),
        State(f.channels.clone()),
        State(f.departments()),
        State(f.config.clone()),
        Fixture::auth(&f.owner),
        Path((f.company.id, desk)),
        submitted(data),
    )
    .await
    .unwrap()
    .0
}

async fn child_department(f: &Fixture) -> crate::entities::department::Department {
    f.departments()
        .create_department(
            f.owner.id,
            f.company.id,
            DepartmentWrite {
                name: "Dispatch".into(),
                slug: "dispatch".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap()
}

async fn by_slug(f: &Fixture) -> Option<Channel> {
    ChannelPersistence::get_by_company_slug_and_channel_slug(
        f.persistence.as_ref(),
        &f.company.slug,
        &"classic-public".into(),
    )
    .await
    .unwrap()
}

async fn read(f: &Fixture, id: Uuid) -> Channel {
    ChannelPersistence::get_by_id(f.persistence.as_ref(), id)
        .await
        .unwrap()
        .unwrap()
}

async fn create(f: &Fixture, data: &BTreeMap<String, String>) -> String {
    create_channel_handler(
        State(f.companies.clone()),
        State(f.channels.clone()),
        State(f.departments()),
        State(f.config.clone()),
        Fixture::auth(&f.owner),
        Path(f.company.id),
        submitted(data),
    )
    .await
    .unwrap()
    .0
}

async fn public_desk(f: &Fixture, department: DepartmentId) -> Channel {
    ChannelPersistence::create(
        f.persistence.as_ref(),
        f.company.id,
        ChannelWrite {
            name: "Public desk".into(),
            slug: "public-desk".into(),
            department_id: Some(department),
            participant_emails: Some(vec!["@public".into(), "named@example.test".into()]),
            enabled: true,
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn access_mode_classic_rendered_create_requires_spam_confirmation_and_selects_its_owner() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    assert!(!f.config.is_spam_scan_enabled());
    let department = child_department(&f).await;
    let html = create_page(&f).await;
    let mut data = controls(&html, "advanced-channel-form");
    assert_eq!(
        data["department_id"],
        department.parent_id.unwrap().to_string()
    );
    assert!(!data.contains_key("public"));
    data.insert("name".into(), "Classic public".into());
    data.insert("slug".into(), "classic-public".into());
    data.insert("department_id".into(), department.id.to_string());
    data.insert("participant_emails".into(), "named@example.test".into());
    check(&html, "advanced-channel-form", &mut data, "public");
    assert!(create(&f, &data).await.contains("Failed to create desk"));
    assert!(by_slug(&f).await.is_none());
    check(
        &html,
        "advanced-channel-form",
        &mut data,
        "confirm_spam_disabled",
    );
    assert!(!create(&f, &data).await.contains("Failed to create desk"));
    let saved = by_slug(&f).await.unwrap();
    assert_eq!(saved.department_id, department.id);
    assert_eq!(saved.access_mode, ChannelAccessMode::Public);
    assert_eq!(saved.participant_emails.unwrap().len(), 2);
}

#[tokio::test]
async fn access_mode_classic_rendered_edit_preserves_public_owner_and_can_close_intake() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let department = child_department(&f).await;
    let desk = public_desk(&f, department.id).await;
    let html = edit_page(&f, desk.id).await;
    let form_id = format!("channel-{}", desk.id);
    let mut data = controls(&html, &form_id);
    assert_eq!(data["public"], "true");
    assert_eq!(data["department_id"], department.id.to_string());
    assert_eq!(data["participant_emails"], "named@example.test");
    data.insert("name".into(), "Name changed only".into());
    assert!(save(&f, desk.id, &data).await.contains("Update failed:"));
    assert_eq!(read(&f, desk.id).await.name, "Public desk");
    check(&html, &form_id, &mut data, "confirm_spam_disabled");
    assert!(!save(&f, desk.id, &data).await.contains("Update failed:"));
    let saved = read(&f, desk.id).await;
    assert_eq!(saved.name, "Name changed only");
    assert_eq!(saved.access_mode, ChannelAccessMode::Public);
    assert_eq!(saved.department_id, department.id);
    let mut closed = controls(&edit_page(&f, desk.id).await, &form_id);
    closed.remove("public");
    closed.remove("confirm_spam_disabled");
    assert!(!save(&f, desk.id, &closed).await.contains("Update failed:"));
    let saved = read(&f, desk.id).await;
    assert_eq!(saved.access_mode, ChannelAccessMode::Members);
    assert_eq!(saved.department_id, department.id);
    assert_eq!(
        saved.participant_emails.unwrap(),
        vec![crate::entities::value_objects::EmailAddress::from(
            "named@example.test"
        )]
    );
}
