use super::*;
use crate::domain::workflow::{ResourceName, RuntimeResourceId};

#[test]
fn unresolved_resources_cannot_enter_admission_configuration() {
    let company_id = company();
    let mut source: Value =
        serde_json::from_str(&registry::example("http.request").unwrap().source).unwrap();
    source["parameter_schema"] = json!({"type":"object"});
    let bundle = Arc::new(
        publication::freeze(
            crate::test_support::workflow::decode(&source.to_string()).unwrap(),
            company_id,
            version(),
            publication::DependencySnapshots::default(),
            vec![],
        )
        .unwrap(),
    );
    let mut config = binding::BindingConfiguration {
        id: WorkflowBindingId::new(Uuid::new_v4()),
        revision: BindingRevision::new(1).unwrap(),
        company_id,
        params: json!({}),
        resources: BTreeMap::new(),
    };
    assert!(binding::ConfiguredBinding::new(config, bundle.clone()).is_err());
    config = binding::BindingConfiguration {
        id: WorkflowBindingId::new(Uuid::new_v4()),
        revision: BindingRevision::new(1).unwrap(),
        company_id,
        params: json!({}),
        resources: BTreeMap::from([(
            ResourceName::parse("wrong").unwrap(),
            RuntimeResourceId::new(Uuid::new_v4()),
        )]),
    };
    assert!(binding::ConfiguredBinding::new(config, bundle).is_err());
}
