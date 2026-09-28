use super::*;
#[path = "test_support.rs"]
mod support;
use support::*;

#[tokio::test]
async fn workflow_lifecycle_source_identity_errors_are_located_for_validate_and_publish() {
    let store = Store::default();
    let mut value: serde_json::Value = serde_json::from_str(&source()).unwrap();
    value["workflow_id"] = serde_json::json!(uuid::Uuid::from_u128(90).to_string());
    definitions(&store)
        .save(draft(value.to_string(), None))
        .await
        .unwrap();
    let service = definitions(&store);
    for error in [
        service
            .validate(target(), publish().expected)
            .await
            .err()
            .unwrap(),
        service.publish(publish()).await.err().unwrap(),
    ] {
        let LifecycleError::Validation(diagnostic) = error else {
            panic!("source diagnostic required")
        };
        assert_eq!(diagnostic.code, "workflow.identity");
        assert_eq!(&*diagnostic.field_path, "/workflow_id");
        assert!(diagnostic.span.line > 0 && diagnostic.span.column > 0);
        assert!(diagnostic.message.contains("draft"));
    }
    assert!(store.memory.lock().unwrap().versions.is_empty());
}

#[tokio::test]
async fn workflow_lifecycle_activation_rechecks_resource_revocation() {
    use crate::application::workflow::binding::{ResourceReadiness, ResourceStatus};
    use crate::domain::workflow::TypeName;
    let store = Store::default();
    let mut body: serde_json::Value = serde_json::from_str(&source()).unwrap();
    body["resources"] = serde_json::json!([{"slot":"service", "kind":"http"}]);
    definitions(&store)
        .save(draft(body.to_string(), None))
        .await
        .unwrap();
    definitions(&store).publish(publish()).await.unwrap();
    let id = RuntimeResourceId::new(uuid::Uuid::from_u128(30));
    store.memory.lock().unwrap().resources.insert(
        id,
        ResourceStatus {
            id,
            company_id: target().company,
            kind: TypeName::parse("http").unwrap(),
            supported_contracts: Default::default(),
            authorized: true,
            readiness: ResourceReadiness::Ready,
        },
    );
    let mut request = configuration(None);
    request
        .resources
        .insert(ResourceName::parse("service").unwrap(), id);
    let saved = bindings(&store).configure(request).await.unwrap();
    let mut racing = store.racing();
    racing.resume_commit = Some(Arc::new(tokio::sync::Barrier::new(2)));
    let service = bindings(&racing);
    let activity = SetBindingActivity {
        target: binding_target(),
        expected: saved.revision,
    };
    let commits = store.memory.lock().unwrap().commits;
    let revocation = async {
        racing.barrier.as_ref().unwrap().wait().await;
        store
            .memory
            .lock()
            .unwrap()
            .resources
            .get_mut(&id)
            .unwrap()
            .readiness = ResourceReadiness::Revoked;
        racing.resume_commit.as_ref().unwrap().wait().await;
    };
    let (result, ()) = tokio::join!(service.activate(activity), revocation);
    assert!(result.is_err());
    let after = store
        .binding(target().company, binding_target().binding)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.revision, saved.revision);
    assert!(!after.active);
    assert_eq!(store.memory.lock().unwrap().commits, commits);
    assert!(bindings(&store).activate(activity).await.is_err());
    assert!(
        !store
            .binding(target().company, binding_target().binding)
            .await
            .unwrap()
            .unwrap()
            .active
    );
    // Revocation must not prevent the administrative stop command.
    assert!(!bindings(&store).deactivate(activity).await.unwrap().active);
}

#[tokio::test]
async fn workflow_lifecycle_publish_archive_race_retains_history_without_late_publication() {
    let store = Store::default();
    definitions(&store)
        .save(draft(source(), None))
        .await
        .unwrap();
    let racing = store.racing();
    let service = definitions(&racing);
    let (publication, archive) = tokio::join!(
        service.publish(publish()),
        service.archive(target(), publish().expected)
    );
    archive.unwrap();
    {
        let memory = store.memory.lock().unwrap();
        assert!(memory.drafts[&(target().company, target().workflow)].archived);
        assert_eq!(memory.versions.len(), usize::from(publication.is_ok()));
    }
    assert!(
        store
            .selectable_version(target().company, publish().version)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn workflow_lifecycle_saves_invalid_source_and_validates_without_publishing() {
    let store = Store::default();
    let service = definitions(&store);
    let saved = service.save(draft("broken: [".into(), None)).await.unwrap();
    assert_eq!(saved.source(), "broken: [");
    assert!(saved.origin().is_none());
    let error = service
        .validate(target(), saved.revision())
        .await
        .err()
        .unwrap();
    assert!(matches!(error, LifecycleError::Validation(d) if d.span.line > 0));
    assert!(store.memory.lock().unwrap().versions.is_empty());
    let updated = service
        .save(draft(source(), Some(saved.revision())))
        .await
        .unwrap();
    service
        .validate(target(), updated.revision())
        .await
        .unwrap();
    assert!(store.memory.lock().unwrap().versions.is_empty());
    assert!(
        service
            .save(draft(source(), Some(saved.revision())))
            .await
            .is_err()
    );
    let mut oversized = draft(
        "x".repeat(compiler::MAX_SOURCE_BYTES + 1),
        Some(updated.revision()),
    );
    assert!(service.save(oversized).await.is_err());
    oversized = draft(source(), Some(updated.revision()));
    oversized.content.title.clear();
    assert!(service.save(oversized).await.is_err());
    assert_eq!(store.memory.lock().unwrap().commits, 2);
}

#[tokio::test]
async fn workflow_lifecycle_publication_replays_original_after_edits_archive_and_directory_failure()
{
    let store = Store::default();
    let original = published(&store).await;
    let service = definitions(&store);
    let updated = service
        .save(draft(format!("{}\n", source()), Some(publish().expected)))
        .await
        .unwrap();
    service.archive(target(), updated.revision()).await.unwrap();
    store.memory.lock().unwrap().fail_capture = true;
    let replay = service.publish(publish()).await.unwrap();
    assert!(Arc::ptr_eq(&replay, &original));
    assert_eq!(store.memory.lock().unwrap().captures, 1);
    assert_eq!(
        store.memory.lock().unwrap().publications[&(target().company, publish().key)]
            .draft
            .source(),
        source()
    );
    assert!(
        store
            .selectable_version(target().company, publish().version)
            .await
            .unwrap()
            .is_none()
    );
    let mut changed = publish();
    changed.version = VersionId::new(uuid::Uuid::from_u128(99));
    assert!(service.publish(changed).await.is_err());
    store.memory.lock().unwrap().revoked = true;
    assert!(service.publish(publish()).await.is_err());
    assert_eq!(store.memory.lock().unwrap().versions.len(), 1);
}

#[tokio::test]
async fn workflow_lifecycle_competing_draft_edits_and_publications_have_one_commit() {
    let store = Store::default();
    definitions(&store)
        .save(draft(source(), None))
        .await
        .unwrap();
    let racing = store.racing();
    let service = definitions(&racing);
    let (a, b) = tokio::join!(
        service.save(draft("first edit".into(), Some(publish().expected))),
        service.save(draft("second edit".into(), Some(publish().expected)))
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert_eq!(store.memory.lock().unwrap().commits, 2);

    let store = Store::default();
    definitions(&store)
        .save(draft(source(), None))
        .await
        .unwrap();
    let racing = store.racing();
    let service = definitions(&racing);
    let (a, b) = tokio::join!(service.publish(publish()), service.publish(publish()));
    assert!(Arc::ptr_eq(&a.unwrap(), &b.unwrap()));
    let memory = store.memory.lock().unwrap();
    assert_eq!(memory.versions.len(), 1);
    assert_eq!(memory.commits, 2);
}

#[tokio::test]
async fn workflow_lifecycle_competing_publication_keys_cannot_reuse_version_identity() {
    let store = Store::default();
    definitions(&store)
        .save(draft(source(), None))
        .await
        .unwrap();
    let racing = store.racing();
    let service = definitions(&racing);
    let mut other = publish();
    other.key = IdempotencyKey::parse("other").unwrap();
    let (a, b) = tokio::join!(service.publish(publish()), service.publish(other));
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert_eq!(store.memory.lock().unwrap().publications.len(), 1);
}

#[tokio::test]
async fn workflow_lifecycle_binding_revisions_activity_cas_and_archive_preserve_snapshots() {
    let store = Store::default();
    let original = published(&store).await;
    let service = bindings(&store);
    let configured = service.configure(configuration(None)).await.unwrap();
    assert!(!configured.active);
    let active = service
        .activate(SetBindingActivity {
            target: binding_target(),
            expected: configured.revision,
        })
        .await
        .unwrap();
    assert!(active.active);
    assert!(
        service
            .deactivate(SetBindingActivity {
                target: binding_target(),
                expected: configured.revision
            })
            .await
            .is_err()
    );
    let mut edit = configuration(Some(active.revision));
    edit.params = serde_json::json!({"changed": true});
    let changed = service.configure(edit).await.unwrap();
    assert!(changed.active);
    assert_eq!(changed.configuration.revision().get(), 2);
    assert_eq!(active.configuration.params(), &serde_json::json!({}));
    assert!(Arc::ptr_eq(active.configuration.bundle(), &original));
    definitions(&store)
        .archive(target(), publish().expected)
        .await
        .unwrap();
    assert!(
        service
            .activate(SetBindingActivity {
                target: binding_target(),
                expected: changed.revision
            })
            .await
            .is_err()
    );
    let inactive = service
        .deactivate(SetBindingActivity {
            target: binding_target(),
            expected: changed.revision,
        })
        .await
        .unwrap();
    assert!(!inactive.active);
    assert_eq!(
        inactive.configuration.params(),
        &serde_json::json!({"changed":true})
    );
}

#[tokio::test]
async fn workflow_lifecycle_competing_binding_activity_and_edits_use_one_state_revision() {
    let store = Store::default();
    published(&store).await;
    let configured = bindings(&store)
        .configure(configuration(None))
        .await
        .unwrap();
    let racing = store.racing();
    let service = bindings(&racing);
    let (activation, edit) = tokio::join!(
        service.activate(SetBindingActivity {
            target: binding_target(),
            expected: configured.revision
        }),
        service.configure(configuration(Some(configured.revision)))
    );
    assert_eq!(
        usize::from(activation.is_ok()) + usize::from(edit.is_ok()),
        1
    );
    let state = store
        .binding(target().company, binding_target().binding)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.revision.get(), 2);
    assert_eq!(store.memory.lock().unwrap().commits, 4);
}

#[tokio::test]
async fn workflow_lifecycle_rejections_leave_state_unchanged() {
    let store = Store::default();
    published(&store).await;
    let service = bindings(&store);
    let mut foreign = configuration(None);
    foreign.target.company = CompanyId::new(uuid::Uuid::from_u128(90));
    assert!(service.configure(foreign).await.is_err());
    let mut resources = configuration(None);
    resources.resources.insert(
        ResourceName::parse("unknown").unwrap(),
        RuntimeResourceId::new(uuid::Uuid::from_u128(91)),
    );
    assert!(service.configure(resources).await.is_err());
    store.memory.lock().unwrap().fail_commit = true;
    assert!(service.configure(configuration(None)).await.is_err());
    assert!(
        definitions(&store)
            .save(draft(source(), Some(publish().expected)))
            .await
            .is_err()
    );
    let state = store.memory.lock().unwrap();
    assert!(state.bindings.is_empty());
    assert_eq!(
        state.drafts[&(target().company, target().workflow)]
            .draft
            .revision()
            .get(),
        1
    );
    assert_eq!(state.commits, 2);
}
