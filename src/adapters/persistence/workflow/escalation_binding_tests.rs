use super::*;
use crate::application::workflow::binding::{EscalationHops, EscalationPolicy};
use crate::application::workflow::reads::WorkflowReads;
use serde_json::json;

fn policy(role: DepartmentRole, hops: u8) -> EscalationPolicy {
    EscalationPolicy {
        notify: role,
        hops: EscalationHops::try_from(hops).unwrap(),
    }
}

#[tokio::test]
async fn workflow_escalation_binding_requires_department_without_role_steps() {
    for on_desk in [false, true] {
        let f = BindingFixture::new(json!([])).await;
        let mut request = f.request(None);
        request.escalation = Some(policy(DepartmentRole::Lead, 2));
        if on_desk {
            request.association =
                RelatedAssociation::Channel(RelatedChannelId::new(desk(&f, None).await));
        }
        let configured = f.service().configure(request).await.unwrap();
        let error = f
            .service()
            .activate(SetBindingActivity {
                target: f.target,
                expected: configured.revision,
            })
            .await
            .err()
            .unwrap();
        assert!(format!("{error:?}").contains("binding.escalation_needs_department"));
        assert!(!f.state().await.active);
    }
}

#[tokio::test]
async fn workflow_escalation_binding_roundtrip_update_and_both_detach_writers() {
    let f = BindingFixture::new(json!([])).await;
    let department = department(&f).await;
    let channel = desk(&f, Some(department)).await;
    let mut request = f.request(None);
    request.association = RelatedAssociation::Channel(RelatedChannelId::new(channel));
    let first = policy(DepartmentRole::Reviewer, 2);
    request.escalation = Some(first);
    let configured = f.service().configure(request).await.unwrap();
    assert_eq!(f.state().await.configuration.escalation(), Some(first));
    let p = &f.fixture.persistence;
    assert_eq!(
        p.binding_summary(f.target.company, f.target.binding)
            .await
            .unwrap()
            .unwrap()
            .escalation,
        Some(first)
    );
    let active = f
        .service()
        .activate(SetBindingActivity {
            target: f.target,
            expected: configured.revision,
        })
        .await
        .unwrap();
    let update = ChannelUpdate {
        company_id: f.target.company.as_uuid(),
        channel_id: channel,
        write: write(None),
    };
    let error = ChannelPersistence::update(p, update.clone())
        .await
        .unwrap_err();
    assert!(format!("{error:?}").contains("binding.escalation_needs_department"));
    let mut tx = p.pool().begin().await.unwrap();
    let error = crate::adapters::persistence::department_desks::set_desk_ownership_on(
        &mut tx,
        f.target.company,
        channel,
        &ChannelOwnershipWrite {
            department_id: Some(None),
            members_only: None,
        },
    )
    .await
    .unwrap_err();
    assert!(format!("{error:?}").contains("binding.escalation_needs_department"));
    tx.rollback().await.unwrap();
    let mut request = f.request(Some(active.revision));
    request.association = RelatedAssociation::Channel(RelatedChannelId::new(channel));
    request.escalation = Some(policy(DepartmentRole::Lead, 1));
    let changed = f.service().configure(request).await.unwrap();
    assert_eq!(
        f.state().await.configuration.escalation(),
        Some(policy(DepartmentRole::Lead, 1))
    );
    let old: serde_json::Value = sqlx::query_scalar("SELECT escalation FROM workflow_binding_revisions WHERE company_id=$1 AND binding_id=$2 AND revision=1")
        .bind(f.target.company.as_uuid()).bind(f.target.binding.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(old, json!({"notify":"reviewer","hops":2}));
    let mut request = f.request(Some(changed.revision));
    request.association = RelatedAssociation::Channel(RelatedChannelId::new(channel));
    request.escalation = None;
    f.service().configure(request).await.unwrap();
    assert_eq!(f.state().await.configuration.escalation(), None);
    assert_eq!(
        p.binding_summary(f.target.company, f.target.binding)
            .await
            .unwrap()
            .unwrap()
            .escalation,
        None
    );
    ChannelPersistence::update(p, update).await.unwrap();
}

#[tokio::test]
async fn workflow_escalation_binding_sql_and_rust_policy_validation_agree() {
    let f = BindingFixture::new(json!([])).await;
    for source in [
        json!(null),
        json!({}),
        json!({"notify":null,"hops":1}),
        json!({"notify":"lead","hops":null}),
        json!({"notify":"lead","hops":"1"}),
        json!({"notify":"lead","hops":0}),
        json!({"notify":"lead","hops":1}),
        json!({"notify":"reviewer","hops":2}),
        json!({"notify":"lead","hops":3}),
        json!({"notify":"lead","hops":1.5}),
        json!({"notify":"lead","hops":1.0}),
        json!({"notify":"lead","hops":1,"extra":true}),
        json!({"notify":"missing","hops":1}),
    ] {
        let (stored, valid): (serde_json::Value, bool) =
            sqlx::query_as("SELECT $1::jsonb,workflow_escalation_policy_valid($1::jsonb)")
                .bind(source)
                .fetch_one(f.fixture.persistence.pool())
                .await
                .unwrap();
        assert_eq!(
            serde_json::from_value::<EscalationPolicy>(stored.clone()).is_ok(),
            valid,
            "{stored}"
        );
    }
}
