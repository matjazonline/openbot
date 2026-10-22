//! Child bundle capture at publication (07-D1 §D1.1, §D1.8): pinned children are read from the
//! publishing company's selectable versions, bounded before they are read.
use super::*;
use crate::adapters::persistence::credentials::CredentialCipher;
use crate::application::use_cases::{
    agent::{AgentPersistence, AgentWrite},
    company::CompanyModelConnectionWrite,
};
use crate::application::workflow::fixtures::Fixture as Template;
use crate::services::test_support::{SCRIPTED_MODEL, SCRIPTED_PROVIDER};
use serde_json::{Value, json};

/// A workflow whose steps call each pinned child in turn.
fn calls(pins: &[(WorkflowId, VersionId)]) -> Value {
    let steps: serde_json::Map<String, Value> = pins
        .iter()
        .enumerate()
        .map(|(index, (workflow, version))| {
            let next = if index + 1 == pins.len() {
                "$end".to_owned()
            } else {
                format!("call{}", index + 1)
            };
            let step = json!({
                "type": "workflow.call",
                "child_workflow_id": workflow.to_string(),
                "child_version_id": version.to_string(),
                "with": {"input": {"ref": "/input"}},
                "routes": {"success": next}
            });
            (format!("call{index}"), step)
        })
        .collect();
    json!({
        "format_version": 1,
        "workflow_id": Uuid::nil().to_string(),
        "input_schema": true,
        "parameter_schema": true,
        "output_schema": true,
        "resources": [],
        "entry": "call0",
        "steps": steps,
        "limits": {"max_steps": 100, "max_context_bytes": 65536}
    })
}

fn leaf() -> Value {
    serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap()
}

async fn attempt(
    f: &Fixture,
    workflow: u128,
    mut source: Value,
) -> LifecycleResult<Arc<PublishedBundle>> {
    let workflow = WorkflowId::new(Uuid::from_u128(workflow));
    source["workflow_id"] = json!(workflow.to_string());
    let version = VersionId::new(Uuid::new_v4());
    f.publish_source(&f.service(), workflow, source.to_string(), version)
        .await
}

/// The draft is refused with `publication.missing` at `step`'s pinned version (any step when
/// `step` is empty).
fn missing(result: LifecycleResult<Arc<PublishedBundle>>, step: &str) {
    match result {
        Err(LifecycleError::Validation(diagnostic)) => {
            assert_eq!(diagnostic.code, "publication.missing");
            let path = &*diagnostic.field_path;
            assert!(path.starts_with(&format!("/steps/{step}")), "{path}");
            assert!(path.ends_with("/child_version_id"), "{path}");
        }
        Err(LifecycleError::Application(error)) => panic!("{error:?}"),
        Ok(_) => panic!("published without its child"),
    }
}

#[tokio::test]
async fn workflow_child_publication_captures_only_selectable_children_of_the_company() {
    let f = Fixture::new().await;
    let child = WorkflowId::new(Uuid::from_u128(0x0710));
    let version = f.publish_workflow(child, leaf()).await;
    // A pinned, selectable child is captured into the parent's bundle.
    let parent = attempt(&f, 0x0720, calls(&[(child, version)]))
        .await
        .unwrap();
    assert_eq!(
        parent.children().keys().copied().collect::<Vec<_>>(),
        [version]
    );
    // An unknown version, or a version of another workflow, is not captured.
    let unknown = VersionId::new(Uuid::new_v4());
    missing(
        attempt(&f, 0x0721, calls(&[(child, unknown)])).await,
        "call0",
    );
    let other = WorkflowId::new(Uuid::from_u128(0x0711));
    missing(
        attempt(&f, 0x0722, calls(&[(other, version)])).await,
        "call0",
    );
    // Another company's version with the same identity is not captured either.
    let foreign = Fixture::in_database(f._db.clone(), "-foreign").await;
    let theirs = foreign
        .publish_workflow(WorkflowId::new(Uuid::from_u128(0x0712)), leaf())
        .await;
    missing(
        attempt(
            &f,
            0x0723,
            calls(&[(WorkflowId::new(Uuid::from_u128(0x0712)), theirs)]),
        )
        .await,
        "call0",
    );
    // An archived child is no longer selectable; the step that pins it is reported.
    let target = DefinitionTarget {
        workflow: child,
        ..f.target
    };
    f.service()
        .archive(target, DraftRevision::new(1).unwrap())
        .await
        .unwrap();
    let archived = attempt(&f, 0x0724, calls(&[(child, version), (child, version)])).await;
    missing(archived, "call0");
}

#[tokio::test]
async fn workflow_child_publication_bounds_pins_and_bytes_before_reading_bundles() {
    let f = Fixture::new().await;
    let pins = |count: usize| -> Vec<(WorkflowId, VersionId)> {
        (0..count)
            .map(|_| {
                (
                    WorkflowId::new(Uuid::new_v4()),
                    VersionId::new(Uuid::new_v4()),
                )
            })
            .collect()
    };
    // 256 distinct pins are read (and reported missing); 257 are refused before any query.
    missing(attempt(&f, 0x0730, calls(&pins(256))).await, "");
    let refused = attempt(&f, 0x0731, calls(&pins(257))).await;
    assert!(
        matches!(
            refused,
            Err(LifecycleError::Application(AppError::BadRequest(_)))
        ),
        "{:?}",
        refused.err()
    );
    // Selected bundles may together hold at most one bundle's bytes.
    let max = i32::try_from(MAX_BUNDLE_BYTES).unwrap();
    child_publication::within_bundle_bound([max]).unwrap();
    child_publication::within_bundle_bound([max - 1, 1]).unwrap();
    for sizes in [vec![max, 1], vec![i32::MAX, i32::MAX]] {
        assert!(matches!(
            child_publication::within_bundle_bound(sizes),
            Err(AppError::BadRequest(_))
        ));
    }
}

/// The production composition: the shipped round's `assistant` is a real agent of the company
/// (no tools or skills), and pinned children are captured from stored versions.
#[tokio::test]
async fn workflow_child_publication_publishes_the_shipped_revision_template_end_to_end() {
    let mut f = Fixture::new().await;
    f.persistence = PostgresPersistence::with_credential_cipher(
        f.persistence.pool().clone(),
        CredentialCipher::for_test(),
    );
    let company = f.target.company.as_uuid();
    AgentPersistence::create(
        &f.persistence,
        company,
        AgentWrite {
            name: "Assistant".into(),
            slug: "assistant".into(),
            provider: Some(SCRIPTED_PROVIDER.into()),
            model: Some(SCRIPTED_MODEL.into()),
            system_prompt: Some("Draft a reply.".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    CompanyPersistence::replace_model_connections_for_user(
        &f.persistence,
        f.target.actor.user_id(),
        company,
        vec![CompanyModelConnectionWrite {
            provider: SCRIPTED_PROVIDER.into(),
            api_key: Some("scripted-key".into()),
            models: vec![SCRIPTED_MODEL.into()],
            is_default: true,
        }],
    )
    .await
    .unwrap();
    let service = DefinitionService::new(
        Preflight,
        f.persistence.clone(),
        WorkflowSourceDecoder,
        child_publication::ChildPublicationDirectory {
            persistence: f.persistence.clone(),
            inner: agent_publication::AgentPublicationDirectory {
                persistence: f.persistence.clone(),
                inner: Preflight,
            },
        },
    );
    let publish = |template: Template, workflow: u128| {
        let (f, service) = (&f, &service);
        async move {
            f.publish_source(
                service,
                WorkflowId::new(Uuid::from_u128(workflow)),
                template.source().to_owned(),
                template.version_id(),
            )
            .await
        }
    };
    // The parent cannot publish before the round it pins exists.
    match publish(Template::RepeatedHumanRevision, 4).await {
        Err(LifecycleError::Validation(diagnostic)) => {
            assert_eq!(diagnostic.code, "publication.missing");
            assert_eq!(&*diagnostic.field_path, "/steps/revisions/child_version_id");
        }
        other => panic!("{:?}", other.map(|bundle| bundle.content_hash().clone())),
    }
    let round = publish(Template::HumanRevisionRound, 5).await.unwrap();
    let parent = WorkflowId::new(Uuid::from_u128(4));
    let mut draft = f.save(Some(DraftRevision::new(1).unwrap()));
    draft.target.workflow = parent;
    draft.content.source = Template::RepeatedHumanRevision.source().to_owned();
    service.save(draft).await.unwrap();
    let mut publication = f.publish();
    publication.target.workflow = parent;
    publication.expected = DraftRevision::new(2).unwrap();
    publication.version = Template::RepeatedHumanRevision.version_id();
    let bundle = service.publish(publication).await.unwrap();
    assert_eq!(bundle.children().len(), 1);
    let pinned = &bundle.children()[&Template::HumanRevisionRound.version_id()];
    assert_eq!(pinned.content_hash(), round.content_hash());
}
