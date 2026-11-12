use super::*;
use crate::application::ports::channel::{ChannelOwnershipWrite, ChannelPersistence, ChannelWrite};
use crate::application::ports::department::DepartmentWrite;
use crate::entities::department::DepartmentRole;
use crate::entities::transport::PrincipalId;

struct RoleAction {
    setup: Setup,
    department: crate::entities::department::DepartmentId,
    admin: Uuid,
    principals: Vec<PrincipalId>,
}

async fn role_action() -> RoleAction {
    let original = Box::pin(protected("http://127.0.0.1:1", Entrance::Http)).await;
    let fixture = Fixture {
        _db: original.fixture._db.clone(),
        persistence: original.fixture.persistence.clone(),
        target: DefinitionTarget {
            workflow: WorkflowId::new(Uuid::new_v4()),
            ..original.fixture.target
        },
    };
    let RoleDepartment {
        department,
        admin,
        principals,
        channel,
    } = role_department(&fixture).await;
    let association =
        RelatedAssociation::Channel(crate::application::workflow::RelatedChannelId::new(channel));
    let version = role_version(&fixture, &original.connection).await;
    let target = Box::pin(bound(
        &fixture,
        &original.connection,
        version,
        Entrance::Http,
        association,
    ))
    .await;
    let claim = Box::pin(claimed_with_key(
        &fixture,
        target,
        association,
        "role-action-run",
    ))
    .await;
    RoleAction {
        setup: Setup {
            fixture,
            connection: original.connection,
            version,
            claim,
            kind: WorkflowStepKind("http.request".into()),
        },
        department,
        admin,
        principals,
    }
}

async fn role_version(fixture: &Fixture, connection: &HttpConnection) -> VersionId {
    let mut source: Value =
        serde_json::from_str(&registry::example("http.request").unwrap().source).unwrap();
    source["workflow_id"] = json!(fixture.target.workflow);
    source["steps"]["start"]["with"]["approval"] = json!({"literal":{
        "reviewers":["reviewer"],"deadline":(chrono::Utc::now()+chrono::Duration::minutes(20)).to_rfc3339()
    }});
    let directory = crate::adapters::persistence::workflow::http::HttpPublicationDirectory {
        persistence: fixture.persistence.clone(),
        selections: vec![
            crate::adapters::persistence::workflow::http::HttpPublicationSelection {
                connection: connection.id,
                slot: Some(ResourceName::parse("service").unwrap()),
                operation: TypeName::parse("http.request").unwrap(),
            },
        ],
    };
    let service = DefinitionService::new(
        Preflight,
        fixture.persistence.clone(),
        WorkflowSourceDecoder,
        directory,
    );
    let mut draft = fixture.save(None);
    draft.content.source = source.to_string();
    service.save(draft).await.unwrap();
    let mut publication = fixture.publish();
    publication.key = IdempotencyKey::parse("role-action-publication").unwrap();
    let version = publication.version;
    service.publish(publication).await.unwrap();
    version
}

struct RoleDepartment {
    department: crate::entities::department::DepartmentId,
    admin: Uuid,
    principals: Vec<PrincipalId>,
    channel: Uuid,
}

async fn role_department(fixture: &Fixture) -> RoleDepartment {
    let owner = fixture.target.actor.user_id();
    let admin = super::super::message_tests::support::member(fixture, "admin").await;
    let principals = vec![
        PrincipalId::new(fixture.principal(owner).await),
        PrincipalId::new(fixture.principal(admin).await),
    ];
    let departments =
        crate::adapters::persistence::test_support::department_use_cases(&fixture.persistence);
    let department = departments
        .create_department(
            owner,
            fixture.target.company.as_uuid(),
            DepartmentWrite {
                name: "Action review".into(),
                slug: "action-review".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap()
        .id;
    for principal in &principals {
        departments
            .add_member(
                owner,
                fixture.target.company.as_uuid(),
                department,
                *principal,
            )
            .await
            .unwrap();
        departments
            .assign_role(
                owner,
                fixture.target.company.as_uuid(),
                department,
                DepartmentRole::Reviewer,
                *principal,
            )
            .await
            .unwrap();
    }
    let channel = ChannelPersistence::create(
        &fixture.persistence,
        fixture.target.company.as_uuid(),
        ChannelWrite {
            name: "Protected action".into(),
            slug: "protected-action".into(),
            enabled: true,
            ownership: Some(ChannelOwnershipWrite {
                department_id: Some(Some(department)),
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    RoleDepartment {
        department,
        admin,
        principals,
        channel,
    }
}

#[tokio::test]
async fn workflow_role_reviewer_protected_action_uses_frozen_people_and_current_approval_authority()
{
    let f = Box::pin(role_action()).await;
    let p = &f.setup.fixture.persistence;
    let owner = f.setup.fixture.target.actor.user_id();
    let company = f.setup.fixture.target.company;
    let departments = crate::adapters::persistence::test_support::department_use_cases(p);
    for principal in &f.principals {
        departments
            .clear_role(
                owner,
                company.as_uuid(),
                f.department,
                DepartmentRole::Reviewer,
                *principal,
            )
            .await
            .unwrap();
    }
    parked(&f.setup).await;
    let decision = view(&f.setup).await;
    assert_eq!(decision.reviewers.len(), 2);
    assert!(
        decision
            .reviewers
            .iter()
            .all(|id| f.principals.contains(&PrincipalId::new(*id)))
    );
    assert!(!decision.reviewers.contains(&owner));
    assert!(!decision.reviewers.contains(&f.admin));
    sqlx::query("UPDATE company_members SET role='member' WHERE company_id=$1 AND user_id=$2")
        .bind(company.as_uuid())
        .bind(f.admin)
        .execute(p.pool())
        .await
        .unwrap();
    let mut revoked = submission(&f.setup, &decision, "approve");
    revoked.actor = WorkflowActor::authenticated(f.admin).unwrap();
    assert!(matches!(
        p.submit_decision(revoked).await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(view(&f.setup).await.state, "waiting");
    p.submit_decision(submission(&f.setup, &decision, "approve"))
        .await
        .unwrap();
    assert_eq!(view(&f.setup).await.state, "completed");
}
