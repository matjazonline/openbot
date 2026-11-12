use super::*;
use crate::adapters::persistence::workflow::escalations::recipients_on;
use crate::application::workflow::binding::{EscalationHops, EscalationPolicy};
use crate::application::workflow::escalation::{NotificationRecipients, NotificationRoleRefusal};
use crate::entities::participant::PrincipalKind;
use std::collections::BTreeSet;

#[path = "escalation_person_tests.rs"]
mod person_notices;

async fn assign(f: &PeopleFixture, department: DepartmentId, principal: PrincipalId) {
    let departments = crate::adapters::persistence::test_support::department_use_cases(f.role.p());
    let actor = f.role.admission.binding.target.actor.user_id();
    departments
        .add_member(actor, f.role.company().as_uuid(), department, principal)
        .await
        .unwrap();
    departments
        .assign_role(
            actor,
            f.role.company().as_uuid(),
            department,
            DepartmentRole::Reviewer,
            principal,
        )
        .await
        .unwrap();
}

async fn selection(f: &PeopleFixture, department: DepartmentId) -> NotificationRecipients {
    let mut db = f.role.p().pool().acquire().await.unwrap();
    let selection = recipients_on(
        &mut db,
        f.role.company(),
        Some(department),
        DepartmentRole::Reviewer,
    )
    .await
    .unwrap();
    let facts = crate::adapters::persistence::workflow::escalations::facts_on(
        &mut db,
        f.role.company(),
        &selection,
    )
    .await
    .unwrap();
    let stored: Value = sqlx::query_scalar(
        "SELECT workflow_escalation_role_selection($1,$2,'reviewer','ancestry')",
    )
    .bind(f.role.company().as_uuid())
    .bind(department.as_uuid())
    .fetch_one(&mut *db)
    .await
    .unwrap();
    assert_eq!(
        facts.canonical(),
        stored,
        "SQL guard and production resolver must seal the same complete snapshot"
    );
    selection
}

async fn maximum_labels(f: &PeopleFixture) {
    let label = format!("x{}", "\u{0001}".repeat(254));
    assert_eq!(label.len(), 255);
    let principals: Vec<_> = f
        .principals
        .iter()
        .map(|principal| principal.as_uuid())
        .collect();
    sqlx::query("UPDATE principals SET display_label=$3 WHERE company_id=$1 AND id=ANY($2)")
        .bind(f.role.company().as_uuid())
        .bind(principals)
        .bind(label)
        .execute(f.role.p().pool())
        .await
        .unwrap();
}

async fn clear_reviewers(f: &PeopleFixture) {
    let departments = crate::adapters::persistence::test_support::department_use_cases(f.role.p());
    for principal in &f.principals {
        departments
            .clear_role(
                f.role.admission.binding.target.actor.user_id(),
                f.role.company().as_uuid(),
                f.role.department,
                DepartmentRole::Reviewer,
                *principal,
            )
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn workflow_escalation_control_real_repair_seals_sixteen_maximum_escaped_labels() {
    let f = Box::pin(PeopleFixture::new(16)).await;
    maximum_labels(&f).await;
    let binding = &f.role.admission.binding;
    let state = binding.state().await;
    let mut request = binding.request(Some(state.revision));
    request.association = RelatedAssociation::Channel(RelatedChannelId::new(f.role.channel));
    request.escalation = Some(EscalationPolicy {
        notify: DepartmentRole::Reviewer,
        hops: EscalationHops::try_from(2).unwrap(),
    });
    binding.service().configure(request).await.unwrap();
    let prepared = f.role.prepared("maximum-escaped-notices").await;
    clear_reviewers(&f).await;
    let failure = f.role.p().admit(&prepared).await.unwrap_err();
    assert!(
        matches!(failure,AppError::WorkflowRole(refusal) if refusal.role==DepartmentRole::Reviewer && refusal.kind==RoleRefusalKind::NoHolder)
    );
    for principal in &f.principals {
        assign(&f, f.role.department, *principal).await;
    }
    let worker = crate::adapters::persistence::test_support::notification_worker(
        f.role.p(),
        Default::default(),
    );
    assert_eq!(worker.drain_once().await.unwrap(), 1);
    #[derive(sqlx::FromRow)]
    struct Captured {
        facts: Value,
        valid: bool,
        bytes: i32,
    }
    let captured:Captured=sqlx::query_as("SELECT recipient_facts AS facts,workflow_escalation_recipient_facts_valid(recipient_facts) AS valid, \
        octet_length(recipient_facts::text) AS bytes FROM workflow_run_escalations WHERE company_id=$1")
        .bind(f.role.company().as_uuid()).fetch_one(f.role.p().pool()).await.unwrap();
    assert!(captured.valid);
    assert!(
        captured.bytes > 8192 && captured.bytes <= 32768,
        "all16 valid maximum255byte escaped labels must fit: {}",
        captured.bytes
    );
    let facts = captured.facts.as_array().unwrap();
    assert_eq!(facts.len(), 16);
    for recipient in facts {
        assert_eq!(recipient["label"].as_str().unwrap().len(), 255);
    }
    let notices: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_escalation_notices WHERE company_id=$1")
            .bind(f.role.company().as_uuid())
            .fetch_one(f.role.p().pool())
            .await
            .unwrap();
    assert_eq!(notices, 16);
    let runs: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_runs WHERE company_id=$1")
        .bind(f.role.company().as_uuid())
        .fetch_one(f.role.p().pool())
        .await
        .unwrap();
    assert_eq!(runs, 0, "the administrative assignment never invents a run");
    let mut seventeen = captured.facts.clone();
    let mut extra = seventeen[0].clone();
    extra["principal"] = json!(Uuid::new_v4());
    seventeen.as_array_mut().unwrap().push(extra);
    let valid: bool = sqlx::query_scalar("SELECT workflow_escalation_recipient_facts_valid($1)")
        .bind(seventeen)
        .fetch_one(f.role.p().pool())
        .await
        .unwrap();
    assert!(
        !valid,
        "the recipient count still fails early at17 after allowing full label encoding"
    );
}

#[tokio::test]
async fn workflow_escalation_notification_actual_role_writers_preserve_mixed_and_multiple_agent_holders()
 {
    let f = Box::pin(PeopleFixture::new(2)).await;
    let (_, second_agent) = create_agent(&f.role.admission.binding, "notice-second-agent").await;
    for principal in [f.role.principal, second_agent] {
        assign(&f, f.role.department, principal).await;
    }
    let NotificationRecipients::Holders {
        department,
        holders,
    } = selection(&f, f.role.department).await
    else {
        panic!("mixed neutral recipients must resolve")
    };
    assert_eq!(department, f.role.department);
    assert_eq!(holders.len(), 4);
    assert_eq!(
        holders
            .iter()
            .filter(|holder| holder.kind == PrincipalKind::Agent)
            .count(),
        2
    );
    assert_eq!(
        holders
            .iter()
            .filter(|holder| holder.kind == PrincipalKind::Person)
            .count(),
        2
    );
    let expected: BTreeSet<_> = f
        .principals
        .iter()
        .copied()
        .chain([f.role.principal, second_agent])
        .collect();
    assert_eq!(
        holders
            .iter()
            .map(|holder| holder.principal)
            .collect::<BTreeSet<_>>(),
        expected
    );
    // The unpublished agent has no delivery route. Neutral selection retains its fact; route
    // eligibility is explicitly assessed later, and does not erase the other capable recipients.
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM agent_active_revisions WHERE company_id=$1 AND agent_id=(SELECT agent_id FROM principals WHERE company_id=$1 AND id=$2)")
        .bind(f.role.company().as_uuid()).bind(second_agent.as_uuid()).fetch_one(f.role.p().pool()).await.unwrap(),0);
}

#[tokio::test]
async fn workflow_escalation_notification_actual_nearest_ineligible_role_shadows_populated_ancestor()
 {
    let f = Box::pin(PeopleFixture::new(2)).await;
    let departments = crate::adapters::persistence::test_support::department_use_cases(f.role.p());
    let child = departments
        .create_department(
            f.role.admission.binding.target.actor.user_id(),
            f.role.company().as_uuid(),
            DepartmentWrite {
                name: "Child".into(),
                slug: "notice-child".into(),
                parent_id: Some(f.role.department),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap()
        .id;
    assert!(
        matches!(selection(&f,child).await,NotificationRecipients::Holders { department,holders }
        if department==f.role.department && holders.len()==2)
    );
    let (_, unpublished) = create_agent(&f.role.admission.binding, "local-unpublished").await;
    assign(&f, child, unpublished).await;
    let NotificationRecipients::Holders {
        department,
        holders,
    } = selection(&f, child).await
    else {
        panic!("nearest populated role shadows ancestor regardless of route eligibility")
    };
    assert_eq!(department, child);
    assert_eq!(holders.len(), 1);
    assert_eq!(holders[0].principal, unpublished);
    assert_eq!(holders[0].kind, PrincipalKind::Agent);
}

#[tokio::test]
async fn workflow_escalation_notification_actual_sixteen_holders_are_complete_and_seventeen_deliver_no_subset()
 {
    let f = Box::pin(PeopleFixture::new(16)).await;
    let NotificationRecipients::Holders {
        department,
        holders,
    } = selection(&f, f.role.department).await
    else {
        panic!("exactly16 is supported")
    };
    assert_eq!(department, f.role.department);
    assert_eq!(
        holders
            .iter()
            .map(|holder| holder.principal)
            .collect::<BTreeSet<_>>(),
        f.principals.iter().copied().collect()
    );
    assign(&f, f.role.department, f.role.principal).await;
    assert_eq!(
        selection(&f, f.role.department).await,
        NotificationRecipients::Refused {
            department: Some(f.role.department),
            reason: NotificationRoleRefusal::RecipientLimit,
        }
    );
    assert_eq!(
        selection(&f, DepartmentId::new(Uuid::new_v4())).await,
        NotificationRecipients::Refused {
            department: None,
            reason: NotificationRoleRefusal::MissingContext,
        }
    );
}
