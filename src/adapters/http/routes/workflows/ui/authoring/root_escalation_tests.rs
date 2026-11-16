use super::*;
use crate::application::workflow::binding::{EscalationHops, EscalationPolicy};
use crate::entities::department::DepartmentRole;

fn selected_escalation(html: &str) -> String {
    let options = html.split_once("name=\"escalation\"").unwrap().1;
    let options = options.split_once("</select>").unwrap().0;
    let selected: Vec<_> = options
        .split("<option value=\"")
        .skip(1)
        .filter(|option| option.split_once('>').unwrap().0.ends_with(" selected"))
        .collect();
    assert_eq!(
        selected.len(),
        1,
        "one explicit stored selection: {options}"
    );
    selected[0].split_once('"').unwrap().0.to_owned()
}

async fn opened(f: &Fixture, workflow: Uuid, binding: Uuid) -> String {
    binding_form(
        f.ui(f.owner),
        Path(workflow),
        Query(BindingFormQuery {
            company_id: f.company,
            binding_id: Some(binding),
        }),
        "/".parse().unwrap(),
    )
    .await
    .unwrap()
    .0
}

async fn created(f: &Fixture, policy: &str) -> Uuid {
    let workflow = Uuid::new_v4();
    let version = f
        .published(
            workflow,
            "Root escalation",
            &with_params(workflow, json!({})),
        )
        .await
        .to_string();
    let channel = f.channel("Root desk", &workflow.to_string(), false).await;
    sqlx::query("UPDATE channels AS desk SET department_id=root.id FROM departments AS root WHERE root.company_id=desk.company_id AND root.is_root AND desk.company_id=$1 AND desk.id=$2")
        .bind(f.company).bind(channel).execute(f.persistence.pool()).await.unwrap();
    let association = format!("channel:{channel}");
    let pane = f.pane(f.owner, workflow, "bindings").await.unwrap();
    let key = field_value(&pane, "form_key");
    let saved = f
        .save_binding(
            f.owner,
            workflow,
            &[
                ("form_key", &key),
                ("version_id", &version),
                ("association", &association),
                ("param:reviewer", "before"),
                ("escalation", policy),
            ],
        )
        .await;
    assert!(saved.contains("Saved binding"), "{saved}");
    workflow
}

async fn round_trip(f: &Fixture, policy: EscalationPolicy) {
    let expected = format!("{}:{}", policy.notify, policy.hops.get());
    let workflow = created(f, &expected).await;
    let usage = f.usage(workflow).await;
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].binding.escalation, Some(policy));
    let binding = usage[0].binding.binding.as_uuid();
    let html = opened(f, workflow, binding).await;
    let selected = selected_escalation(&html);
    assert_eq!(selected, expected);
    let revision = field_value(&html, "expected_state_revision");
    let version = usage[0].binding.version.to_string();
    let saved = f
        .save_binding(
            f.owner,
            workflow,
            &[
                ("binding_id", &binding.to_string()),
                ("expected_state_revision", &revision),
                ("version_id", &version),
                ("param:reviewer", "after"),
                ("escalation", &selected),
            ],
        )
        .await;
    assert!(saved.contains("Saved binding"), "{saved}");
    let stored = f.usage(workflow).await;
    assert_eq!(stored[0].binding.params["reviewer"], "after");
    assert_eq!(stored[0].binding.escalation, Some(policy));
    assert_eq!(
        selected_escalation(&opened(f, workflow, binding).await),
        expected
    );
}

#[tokio::test]
async fn root_membership_escalation_browser_create_and_edit_preserve_all_roles_and_hop_bounds() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    for role in DepartmentRole::ALL {
        for hops in [1, 2] {
            round_trip(
                &f,
                EscalationPolicy {
                    notify: role,
                    hops: EscalationHops::try_from(hops).unwrap(),
                },
            )
            .await;
        }
    }
}

async fn refused(f: &Fixture, workflow: Uuid, value: &str) {
    let before = f.usage(workflow).await;
    let binding = &before[0].binding;
    let version = binding.version.to_string();
    let revision = binding.state_revision.to_string();
    let id = binding.binding.to_string();
    let fields = [
        ("version_id", version.as_str()),
        ("param:reviewer", "must-not-save"),
        ("escalation", value),
    ];
    let mut creating = fields.to_vec();
    creating.push(("form_key", value));
    let mut editing = fields.to_vec();
    editing.extend([
        ("binding_id", id.as_str()),
        ("expected_state_revision", revision.as_str()),
    ]);
    for submitted in [creating, editing] {
        let html = f.save_binding(f.owner, workflow, &submitted).await;
        assert!(
            html.contains("Choose a role and one or two escalation hops."),
            "{html}"
        );
        let stored = f.usage(workflow).await;
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].binding.escalation, binding.escalation);
        assert_eq!(stored[0].binding.params, binding.params);
        assert_eq!(stored[0].binding.state_revision, binding.state_revision);
    }
}

#[tokio::test]
async fn root_membership_escalation_browser_refuses_unknown_roles_and_invalid_hops_without_writing()
{
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = created(&f, "owner:1").await;
    for value in [
        "missing:1",
        "owner:0",
        "owner:3",
        "owner:01",
        "admin:-1",
        "admin:2:1",
        "admin:",
        "reviewer:foo",
    ] {
        refused(&f, workflow, value).await;
    }
}
