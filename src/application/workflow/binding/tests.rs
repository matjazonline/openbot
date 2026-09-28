use super::*;
use crate::adapters::workflow_source::decode;
use crate::application::workflow::{publication, registry};
use crate::domain::workflow::{TypeName, VersionId};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Mutex;
use uuid::Uuid;

fn company() -> CompanyId {
    CompanyId::new(Uuid::from_u128(1))
}
fn resource() -> RuntimeResourceId {
    RuntimeResourceId::new(Uuid::from_u128(2))
}
fn name(s: &str) -> TypeName {
    TypeName::parse(s).unwrap()
}
fn bundle() -> Arc<PublishedBundle> {
    let mut source: Value =
        serde_json::from_str(&registry::example("http.request").unwrap().source).unwrap();
    source["resources"][0]["contract"] = json!("http.json");
    source["parameter_schema"] =
        json!({"type":"object","required":["count"],"properties":{"count":{"type":"integer"}}});
    Arc::new(
        publication::freeze(
            decode(&source.to_string()).unwrap(),
            company(),
            VersionId::new(Uuid::from_u128(3)),
            publication::DependencySnapshots::default(),
            vec![],
        )
        .unwrap(),
    )
}
fn configuration() -> BindingConfiguration {
    BindingConfiguration {
        id: WorkflowBindingId::new(Uuid::from_u128(4)),
        revision: BindingRevision::new(1).unwrap(),
        company_id: company(),
        params: json!({"count":1}),
        resources: BTreeMap::from([(ResourceName::parse("service").unwrap(), resource())]),
    }
}
fn status() -> ResourceStatus {
    ResourceStatus {
        id: resource(),
        company_id: company(),
        kind: name("http"),
        supported_contracts: [name("http.json")].into(),
        authorized: true,
        readiness: ResourceReadiness::Ready,
    }
}
fn actor() -> WorkflowActor {
    WorkflowActor::authenticated(Uuid::from_u128(5)).unwrap()
}

struct Directory(Mutex<Option<ResourceStatus>>);
#[async_trait]
impl ResourceDirectory for Directory {
    async fn inspect(
        &self,
        company_id: CompanyId,
        user: WorkflowActor,
        id: RuntimeResourceId,
    ) -> AppResult<Option<ResourceStatus>> {
        assert_eq!(company_id, company());
        assert_eq!(user.user_id(), actor().user_id());
        assert_eq!(id, resource());
        Ok(self.0.lock().unwrap().clone())
    }
}

#[test]
fn workflow_binding_checks_ownership_params_and_exact_slots() {
    let bundle = bundle();
    for case in 0..7 {
        let mut config = configuration();
        match case {
            0 => config.company_id = CompanyId::new(Uuid::nil()),
            1 => config.params = json!({"count":"1"}),
            2 => config.params = Value::Null,
            3 => config.params = json!({}),
            4 => config.resources.clear(),
            5 => {
                config
                    .resources
                    .insert(ResourceName::parse("extra").unwrap(), resource());
            }
            _ => {
                config.resources.clear();
                config
                    .resources
                    .insert(ResourceName::parse("wrong").unwrap(), resource());
            }
        }
        let error = ConfiguredBinding::new(config, bundle.clone())
            .err()
            .unwrap();
        assert!(error.span.line > 0);
    }
    let mut config = configuration();
    config.params = json!({"count":1,"oversized":"x".repeat(65_537)});
    assert_eq!(
        ConfiguredBinding::new(config, bundle).err().unwrap().code,
        "context.limit"
    );
    assert!(BindingRevision::new(0).is_none());
}

#[test]
fn workflow_binding_revisions_retain_independent_snapshots() {
    let frozen = bundle();
    let old = ConfiguredBinding::new(configuration(), frozen.clone()).unwrap();
    let mut new = configuration();
    new.revision = BindingRevision::new(2).unwrap();
    new.params["count"] = json!(2);
    new.resources.insert(
        ResourceName::parse("service").unwrap(),
        RuntimeResourceId::new(Uuid::from_u128(9)),
    );
    let new = ConfiguredBinding::new(new, frozen.clone()).unwrap();
    assert_eq!(old.revision().get(), 1);
    assert_eq!(new.revision().get(), 2);
    assert_eq!(old.params()["count"], 1);
    assert_eq!(new.params()["count"], 2);
    assert_ne!(old.resources(), new.resources());
    assert!(Arc::ptr_eq(old.bundle(), &frozen));
}

#[tokio::test]
async fn workflow_binding_checks_current_resource_identity_access_and_capabilities() {
    let binding = ConfiguredBinding::new(configuration(), bundle()).unwrap();
    for case in 0..9 {
        let mut current = status();
        match case {
            0 => current.id = RuntimeResourceId::new(Uuid::nil()),
            1 => current.company_id = CompanyId::new(Uuid::nil()),
            2 => current.authorized = false,
            3 => current.readiness = ResourceReadiness::Revoked,
            4 => current.kind = name("mcp"),
            5 => current.supported_contracts.clear(),
            6 => current.readiness = ResourceReadiness::Unavailable,
            7 => {
                current.supported_contracts =
                    (0..257).map(|n| name(&format!("contract{n}"))).collect()
            }
            _ => {}
        }
        let directory = Directory(Mutex::new((case != 8).then_some(current)));
        let error = binding
            .check_readiness(actor(), &directory)
            .await
            .unwrap_err();
        match case {
            0..=3 | 8 => assert!(matches!(error, AppError::NotFound(_))),
            4 | 5 | 7 => assert!(matches!(error, AppError::BadRequest(_))),
            _ => assert!(matches!(error, AppError::Conflict(_))),
        }
    }
}

#[tokio::test]
async fn workflow_binding_rechecks_revocation_and_propagates_directory_outages() {
    let binding = ConfiguredBinding::new(configuration(), bundle()).unwrap();
    let directory = Directory(Mutex::new(Some(status())));
    binding.check_readiness(actor(), &directory).await.unwrap();
    directory.0.lock().unwrap().as_mut().unwrap().readiness = ResourceReadiness::Revoked;
    assert!(matches!(
        binding.check_readiness(actor(), &directory).await,
        Err(AppError::NotFound(_))
    ));
    struct Outage;
    #[async_trait]
    impl ResourceDirectory for Outage {
        async fn inspect(
            &self,
            _: CompanyId,
            _: WorkflowActor,
            _: RuntimeResourceId,
        ) -> AppResult<Option<ResourceStatus>> {
            Err(AppError::Database("directory unavailable".into()))
        }
    }
    assert!(matches!(
        binding.check_readiness(actor(), &Outage).await,
        Err(AppError::Database(_))
    ));
}
