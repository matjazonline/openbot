//! A published workflow and an active binding to schedule, for tests outside this module.
use super::*;
use crate::adapters::workflow_source::WorkflowSourceDecoder;
use crate::application::workflow::registry;

/// Publication with no dependencies: the `data.map` example captures nothing.
struct NoDependencies;

#[async_trait]
impl PublicationDirectory for NoDependencies {
    async fn capture(
        &self,
        _: WorkflowActor,
        _: &CompanyWorkflowDraft,
    ) -> AppResult<PublicationDependencies> {
        Ok(PublicationDependencies {
            snapshots: DependencySnapshots::default(),
            children: vec![],
        })
    }
}

fn authorizer(
    p: &PostgresPersistence,
) -> LifecycleAuthorizer<PostgresPersistence, PostgresPersistence, PostgresPersistence> {
    LifecycleAuthorizer::new(p.clone(), p.clone(), p.clone())
}

/// The `data.map` example; `input_schema` replaces its accept-anything input schema.
pub(crate) fn source(input_schema: Option<serde_json::Value>) -> String {
    let mut source: serde_json::Value =
        serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
    if let Some(schema) = input_schema {
        source["input_schema"] = schema;
    }
    source.to_string()
}

/// Publish `source` as a new workflow of `company`, by `owner`.
pub(crate) async fn publish(
    p: &PostgresPersistence,
    company: Uuid,
    owner: Uuid,
    source: String,
) -> VersionId {
    let workflow = WorkflowId::new(Uuid::new_v4());
    let mut source: serde_json::Value = serde_json::from_str(&source).unwrap();
    source["workflow_id"] = serde_json::json!(workflow.to_string());
    let target = DefinitionTarget {
        company: CompanyId::new(company),
        actor: WorkflowActor::authenticated(owner).unwrap(),
        workflow,
    };
    let service = DefinitionService::new(
        authorizer(p),
        p.clone(),
        WorkflowSourceDecoder,
        NoDependencies,
    );
    service
        .save(SaveDraft {
            target,
            expected: None,
            content: DraftContent {
                title: "Scheduled report".into(),
                description: "Runs on a schedule".into(),
                source: source.to_string(),
            },
        })
        .await
        .unwrap();
    let version = VersionId::new(Uuid::new_v4());
    service
        .publish(PublishDraft {
            target,
            expected: DraftRevision::new(1).unwrap(),
            version,
            key: IdempotencyKey::parse(format!("publish-{}", workflow.as_uuid().simple())).unwrap(),
        })
        .await
        .unwrap();
    version
}

/// Configure and activate a binding of `version`, by `actor`, running as `run_as` (`None`: the
/// actor).
pub(crate) async fn bind(
    p: &PostgresPersistence,
    company: Uuid,
    actor: Uuid,
    version: VersionId,
    association: RelatedAssociation,
    run_as: Option<Uuid>,
) -> WorkflowBindingId {
    let bindings = BindingService::new(authorizer(p), p.clone(), p.clone(), p.clone());
    let target = BindingTarget {
        company: CompanyId::new(company),
        actor: WorkflowActor::authenticated(actor).unwrap(),
        binding: WorkflowBindingId::new(Uuid::new_v4()),
    };
    let state = bindings
        .configure(ConfigureBinding {
            target,
            expected: None,
            association,
            version,
            params: serde_json::json!({}),
            resources: Default::default(),
            run_as: run_as.map(|user| WorkflowActor::authenticated(user).unwrap()),
            message_trigger: None,
        })
        .await
        .unwrap();
    bindings
        .activate(SetBindingActivity {
            target,
            expected: state.revision,
        })
        .await
        .unwrap();
    target.binding
}

/// An active company binding of a published workflow that accepts the schedule input, run as
/// `owner`.
pub(crate) async fn schedulable_binding(
    p: &PostgresPersistence,
    company: Uuid,
    owner: Uuid,
) -> Uuid {
    let version = publish(p, company, owner, source(None)).await;
    bind(
        p,
        company,
        owner,
        version,
        RelatedAssociation::Company,
        None,
    )
    .await
    .as_uuid()
}

/// The schedule use cases as production composes them, on `p`.
pub(crate) fn schedule_use_cases(
    p: &PostgresPersistence,
) -> crate::application::use_cases::schedule::ScheduleUseCases {
    use crate::application::workflow::schedule_admission::ScheduleMaterializer;
    let service = WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), authorizer(p));
    crate::application::use_cases::schedule::ScheduleUseCases::new(
        Arc::new(p.clone()),
        Arc::new(authorizer(p)),
        Arc::new(ScheduleMaterializer::new(p.clone(), service)),
    )
}
