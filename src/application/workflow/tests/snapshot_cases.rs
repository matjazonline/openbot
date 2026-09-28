use super::*;
use crate::domain::workflow::{ResourceName, RuntimeResourceId};

#[tokio::test]
async fn source_alias_selects_original_configuration_after_binding_removal() {
    let company_id = company();
    let logical = version();
    let store = MemoryStore::new(resource_binding(company_id, logical, 1));
    let message_id = CanonicalMessageId::random();
    let source = TriggerSource::Message { message_id };
    store
        .state
        .lock()
        .unwrap()
        .source_messages
        .insert((company_id, message_id), RelatedAssociation::Company);
    let source_request = |key: &str, input| {
        let mut command = request(company_id, logical, key, input);
        command.trigger =
            TriggerRef::new(company_id, trigger_id_for_key(key), source.clone()).unwrap();
        command
    };
    let svc = service(&store);
    let AdmissionResult::Created(run) = svc
        .admit(source_request("original", json!(7)))
        .await
        .unwrap()
    else {
        panic!("created")
    };
    // Later edits and removal cannot replace an already admitted source's
    // selected parameters/resources or make that source unselectable.
    store.state.lock().unwrap().versions.insert(
        (company_id, logical),
        resource_binding(company_id, logical, 2),
    );
    assert_eq!(
        svc.admit(source_request("alias", json!(7))).await.unwrap(),
        AdmissionResult::Replayed(run)
    );
    store.state.lock().unwrap().versions.clear();
    assert_eq!(
        svc.admit(source_request("after-removal", json!(7)))
            .await
            .unwrap(),
        AdmissionResult::Replayed(run)
    );
    assert_eq!(
        svc.admit(source_request("alias", json!(8))).await.unwrap(),
        AdmissionResult::Conflict
    );
    let state = store.state.lock().unwrap();
    assert_eq!(state.runs.len(), 1);
    assert_eq!(state.jobs.len(), 1);
    assert_eq!(state.admissions.len(), 3);
    let original =
        &state.admissions[&(company_id, IdempotencyKey::parse("original").unwrap())].binding;
    let alias = &state.admissions[&(company_id, IdempotencyKey::parse("alias").unwrap())].binding;
    assert!(Arc::ptr_eq(alias, original));
}

fn resource_binding(
    company: CompanyId,
    logical: VersionId,
    revision: u64,
) -> Arc<binding::ConfiguredBinding> {
    let mut source: Value =
        serde_json::from_str(&registry::example("http.request").unwrap().source).unwrap();
    source["input_schema"] = json!({"type":"integer"});
    source["parameter_schema"] =
        json!({"type":"object","required":["count"],"properties":{"count":{"type":"integer"}}});
    let bundle = Arc::new(
        publication::freeze(
            crate::test_support::workflow::decode(&source.to_string()).unwrap(),
            company,
            version(),
            publication::DependencySnapshots::default(),
            vec![],
        )
        .unwrap(),
    );
    Arc::new(
        binding::ConfiguredBinding::new(
            binding::BindingConfiguration {
                id: WorkflowBindingId::new(logical.as_uuid()),
                revision: BindingRevision::new(revision).unwrap(),
                company_id: company,
                params: json!({"count":revision}),
                resources: BTreeMap::from([(
                    ResourceName::parse("service").unwrap(),
                    RuntimeResourceId::new(Uuid::new_v4()),
                )]),
            },
            bundle,
        )
        .unwrap(),
    )
}

#[tokio::test]
async fn admission_pins_binding_bundle_params_resources_and_replay_ignores_later_configuration() {
    let company = company();
    let logical = version();
    let old = resource_binding(company, logical, 1);
    let store = MemoryStore::new(old.clone());
    let svc = service(&store);
    let first = svc
        .admit(request(company, logical, "original", json!(7)))
        .await
        .unwrap();
    let AdmissionResult::Created(original) = first else {
        panic!("created")
    };
    let next = resource_binding(company, logical, 2);
    assert_ne!(old.bundle().content_hash(), next.bundle().content_hash());
    assert_ne!(old.resources(), next.resources());
    store
        .state
        .lock()
        .unwrap()
        .versions
        .insert((company, logical), next.clone());
    assert_eq!(
        svc.admit(request(company, logical, "original", json!(7)))
            .await
            .unwrap(),
        AdmissionResult::Replayed(original)
    );
    assert!(matches!(
        svc.admit(request(company, logical, "later", json!(8)))
            .await
            .unwrap(),
        AdmissionResult::Created(_)
    ));
    {
        let state = store.state.lock().unwrap();
        let saved = &state.admissions[&(company, IdempotencyKey::parse("original").unwrap())];
        assert!(Arc::ptr_eq(&saved.binding, &old));
        assert_eq!(saved.binding.revision().get(), 1);
        assert_eq!(saved.params, json!({"count":1}));
        assert_eq!(saved.input, json!(7));
        let later = &state.admissions[&(company, IdempotencyKey::parse("later").unwrap())];
        assert!(Arc::ptr_eq(&later.binding, &next));
        assert_eq!(later.binding.revision().get(), 2);
    }
    // Removing the selectable head models deactivation/archive; an exact replay
    // still returns its saved run, but new admission cannot select the old version.
    store.state.lock().unwrap().versions.clear();
    assert_eq!(
        svc.admit(request(company, logical, "original", json!(7)))
            .await
            .unwrap(),
        AdmissionResult::Replayed(original)
    );
    assert!(matches!(
        svc.admit(request(company, logical, "new", json!(9))).await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(
        svc.admit(request(company, logical, "original", json!(10)))
            .await
            .unwrap(),
        AdmissionResult::Conflict
    );
    store.state.lock().unwrap().fail_access = true;
    assert!(matches!(
        svc.admit(request(company, logical, "original", json!(7)))
            .await,
        Err(AppError::Database(_))
    ));
    assert_eq!(store.state.lock().unwrap().jobs.len(), 2);
}

#[tokio::test]
async fn admission_schema_validation_and_stale_selection_fail_without_writes() {
    let company = company();
    let logical = version();
    let old = resource_binding(company, logical, 1);
    let store = MemoryStore::new(old);
    let svc = service(&store);
    assert!(matches!(
        svc.admit(request(company, logical, "bad", json!("7")))
            .await,
        Err(AppError::BadRequest(_))
    ));
    let next = resource_binding(company, logical, 2);
    store.state.lock().unwrap().replace_on_admit = Some(next.clone());
    assert!(matches!(
        svc.admit(request(company, logical, "stale", json!(7)))
            .await,
        Err(AppError::Conflict(_))
    ));
    {
        let state = store.state.lock().unwrap();
        assert!(state.runs.is_empty());
        assert!(state.admissions.is_empty());
        assert!(state.jobs.is_empty());
    }
    assert!(matches!(
        svc.admit(request(company, logical, "stale", json!(7)))
            .await
            .unwrap(),
        AdmissionResult::Created(_)
    ));
    let state = store.state.lock().unwrap();
    let saved = state.admissions.values().next().unwrap();
    assert!(Arc::ptr_eq(&saved.binding, &next));
}
