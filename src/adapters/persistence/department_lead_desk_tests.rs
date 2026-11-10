//! The department desk against PostgreSQL (01-departments p03 B2, 01DEP-P03-D8/D9):
//! `department_desk_` for the desk a department is created with and how it follows its lead, and
//! `department_lead_desk_competing_` for lead changes that race.
use super::*;
use crate::application::ports::channel::{ChannelOwnershipWrite, ChannelPersistence, ChannelWrite};
use crate::application::workflow::compiler::agent_reference_steps;
use crate::application::workflow::templates::REPLY_AGENT_STEP;

/// The department's `department:{id}` record.
async fn record(f: &Departments, department: DepartmentId) -> serde_json::Value {
    sqlx::query_scalar(
        "SELECT result FROM workflow_provisioning_commands \
         WHERE company_id = $1 AND command_key = $2",
    )
    .bind(f.company)
    .bind(format!("department:{department}"))
    .fetch_one(f.pool())
    .await
    .expect("the department desk is recorded")
}

fn uuid(value: &serde_json::Value) -> Option<Uuid> {
    value.as_str().and_then(|id| Uuid::parse_str(id).ok())
}

/// The desk: primary address, name, owning department, members-only.
async fn desk_row(f: &Departments, desk: Uuid) -> (String, String, Option<Uuid>, bool) {
    sqlx::query_as(
        "SELECT slug.slug::text, channel.name, channel.department_id, channel.members_only \
           FROM channels AS channel \
           JOIN channel_slugs AS slug ON slug.channel_id = channel.id AND slug.is_primary \
          WHERE channel.company_id = $1 AND channel.id = $2",
    )
    .bind(f.company)
    .bind(desk)
    .fetch_one(f.pool())
    .await
    .unwrap()
}

/// What a binding is now: active, configuration revision, run-as, and the agent its version's
/// reply step names.
#[derive(Debug, PartialEq)]
struct Bound {
    active: bool,
    configuration: i64,
    run_as: Uuid,
    agent: Option<String>,
}

async fn bound(f: &Departments, binding: Uuid) -> Bound {
    let (active, configuration, run_as, source): (bool, i64, Uuid, String) = sqlx::query_as(
        "SELECT binding.active, binding.configuration_revision, revision.run_as_user_id, \
                version.source \
           FROM workflow_bindings AS binding \
           JOIN workflow_binding_revisions AS revision \
             ON revision.company_id = binding.company_id AND revision.binding_id = binding.id \
            AND revision.revision = binding.configuration_revision \
           JOIN workflow_versions AS version \
             ON version.company_id = revision.company_id AND version.id = revision.version_id \
          WHERE binding.company_id = $1 AND binding.id = $2",
    )
    .bind(f.company)
    .bind(binding)
    .fetch_one(f.pool())
    .await
    .unwrap();
    let agent = agent_reference_steps(
        &crate::adapters::workflow_source::WorkflowSourceDecoder,
        &source,
    )
    .unwrap()
    .into_iter()
    .find(|step| step.id.as_str() == REPLY_AGENT_STEP)
    .and_then(|step| step.key)
    .map(|key| key.as_str().to_owned());
    Bound {
        active,
        configuration,
        run_as,
        agent,
    }
}

/// The desk's selected responder and the configuration revision it is pinned to.
async fn selected(f: &Departments, desk: Uuid) -> Option<(Option<Uuid>, Option<i64>)> {
    sqlx::query_as(
        "SELECT binding_id, binding_revision FROM channel_reply_settings \
         WHERE company_id = $1 AND channel_id = $2",
    )
    .bind(f.company)
    .bind(desk)
    .fetch_optional(f.pool())
    .await
    .unwrap()
}

/// An agent's own default desk as it stands: desk, binding, the binding's state and the desk's
/// selection. Department-desk provisioning and re-leading never change it (plan 03 focused
/// checks; 01DEP-P03-B2-F2).
async fn own_desk(
    f: &Departments,
    principal: PrincipalId,
) -> (Uuid, Uuid, Bound, Option<(Option<Uuid>, Option<i64>)>) {
    let agent: Uuid = sqlx::query_scalar("SELECT agent_id FROM principals WHERE id = $1")
        .bind(principal.as_uuid())
        .fetch_one(f.pool())
        .await
        .unwrap();
    let record: serde_json::Value = sqlx::query_scalar(
        "SELECT result FROM workflow_provisioning_commands \
         WHERE company_id = $1 AND command_key = $2",
    )
    .bind(f.company)
    .bind(format!("agent:{agent}"))
    .fetch_one(f.pool())
    .await
    .unwrap();
    let desk = uuid(&record["channel"]["id"]).unwrap();
    let binding = uuid(&record["binding_id"]).unwrap();
    (
        desk,
        binding,
        bound(f, binding).await,
        selected(f, desk).await,
    )
}

async fn desk_bindings(f: &Departments, desk: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM workflow_bindings WHERE company_id = $1 AND channel_id = $2",
    )
    .bind(f.company)
    .bind(desk)
    .fetch_one(f.pool())
    .await
    .unwrap()
}

async fn create_led(
    f: &Departments,
    name: &str,
    lead: Option<PrincipalId>,
) -> AppResult<Department> {
    f.use_cases
        .create_department(f.owner, f.company, f.write(name, None), lead)
        .await
}

async fn lead(
    f: &Departments,
    by: Uuid,
    department: DepartmentId,
    principal: PrincipalId,
) -> AppResult<()> {
    f.use_cases
        .assign_role(by, f.company, department, DepartmentRole::Lead, principal)
        .await
}

/// 03B.1: with an agent lead, the desk at the department's slug and name answers through the
/// autonomous workflow on that agent, selected and active, owned by the department and open to
/// the company; the record names desk and binding.
#[tokio::test]
async fn department_lead_desk_with_an_agent_lead_answers_on_it() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let own = own_desk(&f, f.agent_principal).await;
    let department = create_led(&f, "Ops Room", Some(f.agent_principal))
        .await
        .unwrap();
    assert_eq!(
        own_desk(&f, f.agent_principal).await,
        own,
        "the lead's own desk is untouched"
    );
    let record = record(&f, department.id).await;
    let desk = uuid(&record["channel"]["id"]).unwrap();
    let binding = uuid(&record["binding_id"]).expect("an agent lead's desk has a binding");
    assert_eq!(
        desk_row(&f, desk).await,
        (
            "ops-room".into(),
            "Ops Room".into(),
            Some(department.id.as_uuid()),
            false
        )
    );
    assert_eq!(
        bound(&f, binding).await,
        Bound {
            active: true,
            configuration: 1,
            run_as: f.owner,
            agent: Some("worker".into())
        }
    );
    assert_eq!(selected(&f, desk).await, Some((Some(binding), Some(1))));
    assert_eq!(
        f.roles(department.id).await,
        vec![DepartmentRoleHolder {
            role: DepartmentRole::Lead,
            principal_id: f.agent_principal
        }]
    );
}

/// 03B.1: with a person lead, or none, the desk is a plain team desk owned by the department; its
/// record names no binding.
#[tokio::test]
async fn department_lead_desk_with_a_person_lead_or_none_is_a_plain_team_desk() {
    let Some(f) = Departments::new().await else {
        return;
    };
    for (name, lead) in [("Front", Some(f.member_principal)), ("Back", None)] {
        let department = create_led(&f, name, lead).await.unwrap();
        let record = record(&f, department.id).await;
        let desk = uuid(&record["channel"]["id"]).unwrap();
        assert_eq!(record["binding_id"], serde_json::Value::Null);
        assert_eq!(
            desk_row(&f, desk).await,
            (
                name.to_lowercase(),
                name.into(),
                Some(department.id.as_uuid()),
                false
            )
        );
        assert_eq!(desk_bindings(&f, desk).await, 0);
        assert_eq!(selected(&f, desk).await, None);
    }
}

/// 03B.1: an address a desk already holds refuses the whole creation, whoever leads.
#[tokio::test]
async fn department_lead_desk_taken_address_refuses_the_department() {
    let Some(f) = Departments::new().await else {
        return;
    };
    ChannelPersistence::create(
        &f.persistence,
        f.company,
        ChannelWrite {
            name: "Taken".into(),
            slug: "taken".into(),
            enabled: true,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let counts = || async {
        sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
            "SELECT (SELECT count(*) FROM departments WHERE company_id = $1), \
                    (SELECT count(*) FROM department_roles WHERE company_id = $1), \
                    (SELECT count(*) FROM channels WHERE company_id = $1), \
                    (SELECT count(*) FROM workflow_provisioning_commands WHERE company_id = $1), \
                    (SELECT count(*) FROM workflow_bindings WHERE company_id = $1)",
        )
        .bind(f.company)
        .fetch_one(f.pool())
        .await
        .unwrap()
    };
    let before = counts().await;
    for lead in [Some(f.agent_principal), Some(f.member_principal), None] {
        let refused = create_led(&f, "Taken", lead).await;
        assert!(
            matches!(&refused, Err(AppError::Conflict(message)) if message.contains("'taken'")),
            "{refused:?}"
        );
        assert_eq!(counts().await, before);
    }
}

/// 03B.2: renaming the department keeps its desk, and the desk still follows the lead; once the
/// desk is detached, a lead change touches no desk.
#[tokio::test]
async fn department_lead_desk_survives_a_rename_and_is_left_once_detached() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let department = create_led(&f, "Care", Some(f.agent_principal))
        .await
        .unwrap();
    let desk = uuid(&record(&f, department.id).await["channel"]["id"]).unwrap();
    f.use_cases
        .update_department(
            f.owner,
            f.company,
            department.id,
            DepartmentEdit {
                name: Some("Customer Care".into()),
                slug: Some("customer-care".into()),
                ..DepartmentEdit::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        desk_row(&f, desk).await.0,
        "care",
        "the desk keeps its address"
    );
    let binding = uuid(&record(&f, department.id).await["binding_id"]).unwrap();
    lead(&f, f.owner, department.id, f.member_principal)
        .await
        .unwrap();
    assert!(
        !bound(&f, binding).await.active,
        "the renamed department's desk followed"
    );

    let mut detach = f.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id = $1 FOR NO KEY UPDATE")
        .bind(f.company)
        .execute(&mut *detach)
        .await
        .unwrap();
    crate::adapters::persistence::department_desks::set_desk_ownership_on(
        &mut detach,
        CompanyId::new(f.company),
        desk,
        &ChannelOwnershipWrite {
            department_id: Some(None),
            members_only: None,
        },
    )
    .await
    .unwrap();
    detach.commit().await.unwrap();
    lead(&f, f.owner, department.id, f.agent_principal)
        .await
        .unwrap();
    assert!(
        !bound(&f, binding).await.active,
        "a detached desk is no longer followed"
    );
    assert_eq!(
        selected(&f, desk).await.map(|selected| selected.0),
        Some(None)
    );
}

/// 03B.3 agent → agent: the same binding, re-pointed to a new version on the new lead, run as the
/// admin who changed the lead, and selected at its new revision.
#[tokio::test]
async fn department_lead_desk_agent_to_agent_repoints_the_same_binding() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let closer = company_agent(&f.persistence, f.company, f.owner, "closer").await;
    let department = create_led(&f, "Sales", Some(f.agent_principal))
        .await
        .unwrap();
    let record = record(&f, department.id).await;
    let desk = uuid(&record["channel"]["id"]).unwrap();
    let binding = uuid(&record["binding_id"]).unwrap();
    let own = (
        own_desk(&f, f.agent_principal).await,
        own_desk(&f, closer).await,
    );
    lead(&f, f.admin, department.id, closer).await.unwrap();
    assert_eq!(
        (
            own_desk(&f, f.agent_principal).await,
            own_desk(&f, closer).await
        ),
        own,
        "neither lead's own desk changes"
    );
    assert_eq!(
        bound(&f, binding).await,
        Bound {
            active: true,
            configuration: 2,
            run_as: f.admin,
            agent: Some("closer".into())
        }
    );
    assert_eq!(selected(&f, desk).await, Some((Some(binding), Some(2))));
    assert_eq!(desk_bindings(&f, desk).await, 1);
    // Leading again with the same agent changes nothing.
    lead(&f, f.admin, department.id, closer).await.unwrap();
    assert_eq!(bound(&f, binding).await.configuration, 2);
}

/// 03B.3 agent → person, person → agent, and back (F6): the owned binding pauses and is
/// unselected, then is re-pointed, reactivated and reselected -- never a second binding; on a
/// plain desk a person → agent change provisions the binding and records it.
#[tokio::test]
async fn department_lead_desk_follows_person_and_agent_leads_with_one_binding() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let own = own_desk(&f, f.agent_principal).await;
    let department = create_led(&f, "Field", Some(f.agent_principal))
        .await
        .unwrap();
    let desk = uuid(&record(&f, department.id).await["channel"]["id"]).unwrap();
    let binding = uuid(&record(&f, department.id).await["binding_id"]).unwrap();
    lead(&f, f.owner, department.id, f.member_principal)
        .await
        .unwrap();
    assert!(!bound(&f, binding).await.active);
    assert_eq!(
        selected(&f, desk).await.map(|selected| selected.0),
        Some(None)
    );
    lead(&f, f.owner, department.id, f.agent_principal)
        .await
        .unwrap();
    assert!(bound(&f, binding).await.active);
    assert_eq!(
        selected(&f, desk).await.map(|selected| selected.0),
        Some(Some(binding))
    );
    assert_eq!(desk_bindings(&f, desk).await, 1);
    assert_eq!(
        own_desk(&f, f.agent_principal).await,
        own,
        "pausing and resuming leave its own desk"
    );

    let plain = create_led(&f, "Plain", Some(f.member_principal))
        .await
        .unwrap();
    let plain_desk = uuid(&record(&f, plain.id).await["channel"]["id"]).unwrap();
    lead(&f, f.owner, plain.id, f.agent_principal)
        .await
        .unwrap();
    let provisioned = uuid(&record(&f, plain.id).await["binding_id"]).expect("recorded");
    assert_eq!(
        bound(&f, provisioned).await,
        Bound {
            active: true,
            configuration: 1,
            run_as: f.owner,
            agent: Some("worker".into())
        }
    );
    assert_eq!(
        selected(&f, plain_desk).await,
        Some((Some(provisioned), Some(1)))
    );
    // Clearing the role pauses it again, even when a user has unselected it first (F9).
    let mut unselect = f.pool().begin().await.unwrap();
    sqlx::query("UPDATE channel_reply_settings SET binding_id = NULL, binding_revision = NULL, contract = NULL, revision = revision + 1 WHERE company_id = $1 AND channel_id = $2")
        .bind(f.company)
        .bind(plain_desk)
        .execute(&mut *unselect)
        .await
        .unwrap();
    unselect.commit().await.unwrap();
    f.use_cases
        .clear_role(
            f.owner,
            f.company,
            plain.id,
            DepartmentRole::Lead,
            f.agent_principal,
        )
        .await
        .unwrap();
    assert!(!bound(&f, provisioned).await.active);
}

/// G2-F2: the binding's run-as follows whoever changes the lead, so a lead set by an admin who
/// has since left the company does not block the next admin's change.
#[tokio::test]
async fn department_lead_desk_run_as_moves_to_the_admin_who_changes_the_lead() {
    use crate::use_cases::company_invite::CompanyInvitePersistence;
    let Some(f) = Departments::new().await else {
        return;
    };
    let (second_admin, _) = teammate(f.pool(), f.company, "admin").await;
    let department = create_led(&f, "Night", Some(f.member_principal))
        .await
        .unwrap();
    lead(&f, f.admin, department.id, f.agent_principal)
        .await
        .unwrap();
    let binding = uuid(&record(&f, department.id).await["binding_id"]).unwrap();
    assert_eq!(bound(&f, binding).await.run_as, f.admin);
    lead(&f, f.admin, department.id, f.owner_principal)
        .await
        .unwrap();
    CompanyInvitePersistence::remove_member(&f.persistence, f.company, f.admin)
        .await
        .unwrap();
    lead(&f, second_admin, department.id, f.agent_principal)
        .await
        .unwrap();
    let now = bound(&f, binding).await;
    assert_eq!((now.active, now.run_as), (true, second_admin));
}

/// D9 refusals: edits the desk does not run (F8), an approval policy, and a desk the admin cannot
/// see each refuse the lead change and leave everything as it was; re-saving the bound content
/// clears the first.
#[tokio::test]
async fn department_lead_desk_refuses_lead_changes_it_cannot_follow() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let closer = company_agent(&f.persistence, f.company, f.owner, "closer").await;
    let department = create_led(&f, "Desk Ops", Some(f.agent_principal))
        .await
        .unwrap();
    let desk = uuid(&record(&f, department.id).await["channel"]["id"]).unwrap();
    let binding = uuid(&record(&f, department.id).await["binding_id"]).unwrap();
    let workflow: Uuid = sqlx::query_scalar(
        "SELECT version.workflow_id FROM workflow_bindings AS binding \
         JOIN workflow_binding_revisions AS revision ON revision.company_id = binding.company_id \
          AND revision.binding_id = binding.id AND revision.revision = binding.configuration_revision \
         JOIN workflow_versions AS version ON version.company_id = revision.company_id \
          AND version.id = revision.version_id WHERE binding.id = $1",
    )
    .bind(binding)
    .fetch_one(f.pool())
    .await
    .unwrap();
    let original: String =
        sqlx::query_scalar("SELECT source FROM workflow_definitions WHERE id = $1")
            .bind(workflow)
            .fetch_one(f.pool())
            .await
            .unwrap();
    let edited = format!("{original}\n# an unpublished edit\n");
    sqlx::query(
        "UPDATE workflow_definitions SET source = $2, revision = revision + 1 WHERE id = $1",
    )
    .bind(workflow)
    .bind(&edited)
    .execute(f.pool())
    .await
    .unwrap();
    let before = bound(&f, binding).await;
    let refused = lead(&f, f.owner, department.id, closer).await;
    assert!(
        matches!(&refused, Err(AppError::BadRequest(message)) if message.contains("edits its desk does not run")),
        "{refused:?}"
    );
    assert_eq!(bound(&f, binding).await, before);
    assert_eq!(
        f.roles(department.id).await[0].principal_id,
        f.agent_principal
    );
    sqlx::query(
        "UPDATE workflow_definitions SET source = $2, revision = revision + 1 WHERE id = $1",
    )
    .bind(workflow)
    .bind(&original)
    .execute(f.pool())
    .await
    .unwrap();
    lead(&f, f.owner, department.id, closer)
        .await
        .expect("restored content re-leads");
    assert_eq!(bound(&f, binding).await.agent.as_deref(), Some("closer"));

    // An approval policy on the desk's replies.
    sqlx::query("UPDATE channel_reply_settings SET approval_policy = $3, revision = revision + 1 WHERE company_id = $1 AND channel_id = $2")
        .bind(f.company)
        .bind(desk)
        .bind(serde_json::to_value(
            crate::application::workflow::channel_reply::ReplyApprovalPolicy::new(vec![f.owner], 3600).unwrap(),
        ).unwrap())
        .execute(f.pool())
        .await
        .unwrap();
    let refused = lead(&f, f.owner, department.id, f.agent_principal).await;
    assert!(
        matches!(&refused, Err(AppError::BadRequest(message)) if message.contains("approve replies")),
        "{refused:?}"
    );
    sqlx::query("UPDATE channel_reply_settings SET approval_policy = NULL, revision = revision + 1 WHERE company_id = $1 AND channel_id = $2")
        .bind(f.company)
        .bind(desk)
        .execute(f.pool())
        .await
        .unwrap();

    // A members-only department desk the admin has no grant on.
    let mut restrict = f.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id = $1 FOR NO KEY UPDATE")
        .bind(f.company)
        .execute(&mut *restrict)
        .await
        .unwrap();
    crate::adapters::persistence::department_desks::set_desk_ownership_on(
        &mut restrict,
        CompanyId::new(f.company),
        desk,
        &ChannelOwnershipWrite {
            department_id: None,
            members_only: Some(true),
        },
    )
    .await
    .unwrap();
    restrict.commit().await.unwrap();
    let refused = lead(&f, f.admin, department.id, f.agent_principal).await;
    assert!(
        matches!(&refused, Err(AppError::BadRequest(message)) if message.contains("cannot see the department desk")),
        "{refused:?}"
    );
    assert_eq!(bound(&f, binding).await.agent.as_deref(), Some("closer"));
}

/// B1 D1 with D9: an agent created as a department's lead answers both its own desk and the
/// department's.
#[tokio::test]
async fn department_lead_desk_follows_an_agent_created_as_its_lead() {
    use crate::use_cases::agent::{AgentPlacement, AgentWrite};
    let Some(f) = Departments::new().await else {
        return;
    };
    let department = create_led(&f, "Growth", Some(f.member_principal))
        .await
        .unwrap();
    crate::adapters::persistence::test_support::company_agents(&f.persistence)
        .create(
            WorkflowActor::authenticated(f.owner).unwrap(),
            CompanyId::new(f.company),
            AgentWrite {
                name: "Grower".into(),
                slug: "grower".into(),
                ..AgentWrite::default()
            },
            Some(AgentPlacement {
                department: department.id,
                role: Some(DepartmentRole::Lead),
            }),
        )
        .await
        .unwrap();
    let binding = uuid(&record(&f, department.id).await["binding_id"]).unwrap();
    assert_eq!(bound(&f, binding).await.agent.as_deref(), Some("grower"));
    let grower: Uuid = sqlx::query_scalar(
        "SELECT principal.id FROM principals AS principal JOIN agents AS agent \
           ON agent.id = principal.agent_id WHERE agent.company_id = $1 AND agent.slug = 'grower'",
    )
    .bind(f.company)
    .fetch_one(f.pool())
    .await
    .unwrap();
    let (desk, own_binding, own, own_selected) = own_desk(&f, PrincipalId::new(grower)).await;
    assert_ne!(own_binding, binding, "its own desk keeps its own binding");
    assert_eq!(
        (own.active, own.agent.as_deref(), own_selected),
        (
            true,
            Some("grower"),
            Some((Some(own_binding), Some(own.configuration)))
        )
    );
    assert_eq!(desk_row(&f, desk).await.0, "grower");
}

/// Decision 9: a department whose agent-led desk is still its own is not deleted; detached, it is.
#[tokio::test]
async fn department_lead_desk_blocks_deleting_its_department_until_detached() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let department = create_led(&f, "Temp", Some(f.agent_principal))
        .await
        .unwrap();
    let desk = uuid(&record(&f, department.id).await["channel"]["id"]).unwrap();
    assert!(matches!(
        f.use_cases
            .delete_department(f.owner, f.company, department.id)
            .await,
        Err(AppError::Conflict(_))
    ));
    let mut detach = f.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id = $1 FOR NO KEY UPDATE")
        .bind(f.company)
        .execute(&mut *detach)
        .await
        .unwrap();
    crate::adapters::persistence::department_desks::set_desk_ownership_on(
        &mut detach,
        CompanyId::new(f.company),
        desk,
        &ChannelOwnershipWrite {
            department_id: Some(None),
            members_only: None,
        },
    )
    .await
    .unwrap();
    detach.commit().await.unwrap();
    f.use_cases
        .delete_department(f.owner, f.company, department.id)
        .await
        .unwrap();
    let records: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_provisioning_commands WHERE company_id = $1 AND command_key = $2",
    )
    .bind(f.company)
    .bind(format!("department:{}", department.id))
    .fetch_one(f.pool())
    .await
    .unwrap();
    assert_eq!(records, 0);
}

/// Two lead changes of one department, the second waiting on the company lock: both apply in
/// turn, one binding stays, and it runs the lead that holds the role.
#[tokio::test]
async fn department_lead_desk_competing_lead_changes_leave_the_holders_agent_answering() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = Arc::new(Departments::on(database.pool.clone()).await);
    let closer = company_agent(&f.persistence, f.company, f.owner, "closer").await;
    let department = create_led(&f, "Race", Some(f.member_principal))
        .await
        .unwrap();
    let blocker = competing::hold_company(&f).await;
    let competitors: Vec<_> = [f.agent_principal, closer]
        .into_iter()
        .map(|principal| {
            let f = f.clone();
            tokio::spawn(async move { lead(&f, f.owner, department.id, principal).await })
        })
        .collect();
    wait_until_backends_are_blocked(f.pool(), 2).await;
    blocker.commit().await.unwrap();
    for competitor in competitors {
        competitor
            .await
            .unwrap()
            .expect("each lead change applies in turn");
    }
    let holder = f.roles(department.id).await[0].principal_id;
    let slug = if holder == closer { "closer" } else { "worker" };
    let record = record(&f, department.id).await;
    let desk = uuid(&record["channel"]["id"]).unwrap();
    let binding = uuid(&record["binding_id"]).unwrap();
    assert_eq!(desk_bindings(&f, desk).await, 1);
    let now = bound(&f, binding).await;
    assert_eq!((now.active, now.agent.as_deref()), (true, Some(slug)));
}

/// 01DEP-P03-B2-F3 (G2-F4): a responder a user chose on the department desk -- their own active,
/// selected binding -- is never re-pointed, paused or unselected by a lead change; the owned
/// binding is still paused when the lead stops being an agent.
#[tokio::test]
async fn department_lead_desk_leaves_a_users_own_responder_alone() {
    use crate::application::workflow::LifecycleAuthorizer;
    use crate::application::workflow::channel_reply::{
        ChannelReplies, ConfigureChannelReply, ConfigureResponder,
    };
    use crate::application::workflow::lifecycle::{
        BindingService, BindingTarget, ConfigureBinding, SetBindingActivity,
    };
    use crate::application::workflow::{RelatedAssociation, RelatedChannelId};
    use crate::domain::workflow::{MessageTrigger, VersionId, WorkflowBindingId};
    let Some(f) = Departments::new().await else {
        return;
    };
    let closer = company_agent(&f.persistence, f.company, f.owner, "closer").await;
    let department = create_led(&f, "Own", Some(f.agent_principal))
        .await
        .unwrap();
    let desk = uuid(&record(&f, department.id).await["channel"]["id"]).unwrap();
    let owned = uuid(&record(&f, department.id).await["binding_id"]).unwrap();
    let version: Uuid = sqlx::query_scalar(
        "SELECT revision.version_id FROM workflow_bindings AS binding \
         JOIN workflow_binding_revisions AS revision ON revision.company_id = binding.company_id \
          AND revision.binding_id = binding.id AND revision.revision = binding.configuration_revision \
         WHERE binding.id = $1",
    )
    .bind(owned)
    .fetch_one(f.pool())
    .await
    .unwrap();
    // A user's own binding of the same published version on the desk, set up through the
    // lifecycle service, then selected as the desk's responder.
    let p = f.persistence.clone();
    let bindings = BindingService::new(
        LifecycleAuthorizer::new(p.clone(), p.clone(), p.clone()),
        p.clone(),
        p.clone(),
        p.clone(),
    );
    let owner = WorkflowActor::authenticated(f.owner).unwrap();
    let target = BindingTarget {
        company: CompanyId::new(f.company),
        actor: owner,
        binding: WorkflowBindingId::new(Uuid::new_v4()),
    };
    let configured = bindings
        .configure(ConfigureBinding {
            target,
            expected: None,
            association: RelatedAssociation::Channel(RelatedChannelId::new(desk)),
            version: VersionId::new(version),
            params: json!({}),
            resources: Default::default(),
            run_as: None,
            message_trigger: Some(MessageTrigger::Direct),
        })
        .await
        .unwrap();
    let active = bindings
        .activate(SetBindingActivity {
            target,
            expected: configured.revision,
        })
        .await
        .unwrap();
    let settings = p
        .channel_reply_settings(
            CompanyId::new(f.company),
            RelatedChannelId::new(desk),
            owner,
        )
        .await
        .unwrap()
        .unwrap();
    p.configure_channel_reply(ConfigureChannelReply {
        company: CompanyId::new(f.company),
        channel: RelatedChannelId::new(desk),
        actor: owner,
        expected: Some(settings.revision),
        responder: Some(ConfigureResponder {
            binding: target.binding,
            expected_binding_revision: active.revision,
            contract: settings.responder.unwrap().contract,
        }),
        approval_policy: None,
    })
    .await
    .unwrap();
    let own = target.binding.as_uuid();
    let chosen = (bound(&f, own).await, selected(&f, desk).await);
    let owned_before = bound(&f, owned).await;

    lead(&f, f.owner, department.id, closer).await.unwrap();
    assert_eq!((bound(&f, own).await, selected(&f, desk).await), chosen);
    assert_eq!(
        bound(&f, owned).await,
        owned_before,
        "agent -> agent leaves both"
    );

    lead(&f, f.owner, department.id, f.member_principal)
        .await
        .unwrap();
    assert_eq!((bound(&f, own).await, selected(&f, desk).await), chosen);
    assert!(!bound(&f, owned).await.active, "the owned binding pauses");
}
