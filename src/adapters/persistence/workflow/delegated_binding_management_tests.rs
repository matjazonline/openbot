//! The ordinary public binding service reuses captured messaging, without root catalog access.
use super::*;
use crate::adapters::persistence::test_support::{delegated_unit, insert_root_member};
use crate::application::ports::channel::{ChannelPersistence, ChannelWrite};
use crate::application::workflow::{LifecycleAuthorizer, WorkflowAuthorization, WorkflowOperation};
use crate::entities::company_member::CompanyAccessRole;
use crate::use_cases::user::UserPersistence;

async fn lead(f: &Fixture) -> WorkflowActor {
    let suffix = Uuid::new_v4().simple().to_string();
    let person = f
        .persistence
        .create_user(&suffix, &format!("{suffix}@example.test"), "hash")
        .await
        .unwrap();
    insert_root_member(
        f.persistence.pool(),
        f.target.company,
        person.id,
        CompanyAccessRole::Member,
        crate::entities::access_level::AccessLevel::PUBLIC,
    )
    .await;
    WorkflowActor::authenticated(person.id).unwrap()
}

async fn message_version(f: &Fixture, desk: Uuid) -> VersionId {
    let mut body: serde_json::Value =
        serde_json::from_str(&registry::example("message.send").unwrap().source).unwrap();
    body["workflow_id"] = json!(f.target.workflow.to_string());
    body["steps"]["start"]["with"] = json!({
        "content":{"literal":{"subject":"Unit status","body":"Scoped work"}},
        "destinations":{"literal":[{"kind":"channel","id":desk.to_string()}]}
    });
    let p = &f.persistence;
    let service = DefinitionService::new(
        LifecycleAuthorizer::new(p.clone(), p.clone(), p.clone()),
        p.clone(),
        WorkflowSourceDecoder,
        super::super::super::step_builtins::StepBuiltinPublicationDirectory {
            persistence: p.clone(),
        },
    );
    let mut draft = f.save(None);
    draft.content.source = body.to_string();
    service.save(draft).await.unwrap();
    let publish = f.publish();
    service.publish(publish.clone()).await.unwrap();
    publish.version
}

async fn managed_desk(f: &Fixture, actor: WorkflowActor) -> Uuid {
    let unit = delegated_unit(&f.persistence, f.target.company, f.target.actor, actor).await;
    ChannelPersistence::create(
        &f.persistence,
        actor,
        f.target.company.as_uuid(),
        ChannelWrite {
            name: "Delegated binding".into(),
            slug: "delegated-binding".into(),
            department_id: Some(unit),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id
}

#[tokio::test]
async fn delegated_admin_binding_service_configures_activates_and_pauses_captured_messaging() {
    let f = Fixture::new().await;
    let p = &f.persistence;
    let actor = lead(&f).await;
    let desk = managed_desk(&f, actor).await;
    let version = message_version(&f, desk).await;
    let association = RelatedAssociation::Channel(RelatedChannelId::new(desk));
    let authorizer = LifecycleAuthorizer::new(p.clone(), p.clone(), p.clone());
    assert!(matches!(
        authorizer
            .authorize(
                f.target.company,
                actor,
                association,
                WorkflowOperation::Inspect
            )
            .await,
        Err(AppError::NotFound(_))
    ));
    assert!(matches!(
        p.inspect(
            f.target.company,
            actor,
            RuntimeResourceId::new(f.target.company.as_uuid())
        )
        .await,
        Err(AppError::NotFound(_))
    ));
    let service = BindingService::new(authorizer, p.clone(), p.clone(), p.clone());
    let target = BindingTarget {
        company: f.target.company,
        actor,
        binding: WorkflowBindingId::new(Uuid::new_v4()),
    };
    let state = service
        .configure(ConfigureBinding {
            target,
            expected: None,
            association,
            version,
            params: json!({}),
            resources: Default::default(),
            run_as: None,
            message_trigger: None,
            escalation: None,
        })
        .await
        .unwrap();
    assert!(
        state
            .configuration
            .bundle()
            .snapshots()
            .tools
            .iter()
            .any(|tool| tool.kind() == crate::application::workflow::publication::ToolKind::Message)
    );
    let active = service
        .activate(SetBindingActivity {
            target,
            expected: state.revision,
        })
        .await
        .unwrap();
    assert!(active.active);
    let paused = service
        .deactivate(SetBindingActivity {
            target,
            expected: active.revision,
        })
        .await
        .unwrap();
    assert!(!paused.active);
    assert_eq!(
        p.binding(target.company, target.binding)
            .await
            .unwrap()
            .unwrap()
            .revision,
        paused.revision
    );
    // Plan06 02B: configure, activate and pause each record the lead's role over the desk.
    let roles: Vec<Option<String>> = sqlx::query_scalar(
        "SELECT acting_role FROM workflow_binding_events WHERE company_id=$1 AND binding_id=$2 \
         ORDER BY state_revision",
    )
    .bind(target.company.as_uuid())
    .bind(target.binding.as_uuid())
    .fetch_all(p.pool())
    .await
    .unwrap();
    assert_eq!(roles.len(), 3, "{roles:?}");
    assert!(
        roles.iter().all(|role| role
            .as_deref()
            .is_some_and(|role| role.starts_with("lead:"))),
        "{roles:?}"
    );
}

#[tokio::test]
async fn delegated_admin_binding_activity_hides_current_and_stale_revisions_from_other_units() {
    use crate::application::workflow::lifecycle::LifecycleError;
    let f = BindingFixture::new(json!([])).await;
    let p = &f.fixture.persistence;
    let state = f.service().configure(f.request(None)).await.unwrap();
    let stranger = lead(&f.fixture).await;
    delegated_unit(p, f.target.company, f.target.actor, stranger).await;
    let real = BindingService::new(
        LifecycleAuthorizer::new(p.clone(), p.clone(), p.clone()),
        p.clone(),
        p.clone(),
        p.clone(),
    );
    let stale = BindingStateRevision::new(state.revision.get() + 1).unwrap();
    for binding in [f.target.binding, WorkflowBindingId::new(Uuid::new_v4())] {
        for expected in [state.revision, stale] {
            let request = SetBindingActivity {
                target: BindingTarget {
                    actor: stranger,
                    binding,
                    ..f.target
                },
                expected,
            };
            assert!(matches!(
                real.activate(request).await,
                Err(LifecycleError::Application(AppError::NotFound(_)))
            ));
            assert!(matches!(
                real.deactivate(request).await,
                Err(LifecycleError::Application(AppError::NotFound(_)))
            ));
        }
    }
    let authorized_stale = SetBindingActivity {
        target: f.target,
        expected: stale,
    };
    assert!(matches!(
        real.activate(authorized_stale).await,
        Err(LifecycleError::Application(AppError::Conflict(_)))
    ));
    assert!(matches!(
        real.deactivate(authorized_stale).await,
        Err(LifecycleError::Application(AppError::Conflict(_)))
    ));
    assert_eq!(
        p.binding(f.target.company, f.target.binding)
            .await
            .unwrap()
            .unwrap()
            .revision,
        state.revision
    );
}

#[tokio::test]
async fn delegated_admin_competing_binding_activity_locks_the_actor_before_the_company() {
    use crate::adapters::persistence::test_support::assert_actor_before_company;
    let f = BindingFixture::new(json!([])).await;
    let state = f.service().configure(f.request(None)).await.unwrap();
    let p = &f.fixture.persistence;
    let real = BindingService::new(
        LifecycleAuthorizer::new(p.clone(), p.clone(), p.clone()),
        p.clone(),
        p.clone(),
        p.clone(),
    );
    let command = f.activity(state.revision).await;
    assert_actor_before_company(
        p.pool(),
        f.target.company.as_uuid(),
        f.target.actor.user_id(),
        p.save_binding(command),
    )
    .await
    .unwrap();
    let active = p
        .binding(f.target.company, f.target.binding)
        .await
        .unwrap()
        .unwrap();
    let paused = real
        .deactivate(SetBindingActivity {
            target: f.target,
            expected: active.revision,
        })
        .await
        .unwrap();
    assert!(!paused.active);
}
