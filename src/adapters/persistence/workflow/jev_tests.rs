use super::*;
use crate::adapters::persistence::credentials::{CredentialCipher, CredentialContext};
use crate::application::workflow::{binding::ResourceDirectory, jev::*, lease::*};
use crate::domain::entities::correlation::CorrelationId;
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Duration};

pub(super) async fn fixture() -> Fixture {
    let mut f = Fixture::new().await;
    f.persistence = PostgresPersistence::with_credential_cipher(
        f.persistence.pool().clone(),
        CredentialCipher::for_test(),
    );
    f
}
fn models(model: &str) -> std::collections::BTreeSet<JevModel> {
    [JevModel::parse(model).unwrap()].into()
}
pub(super) async fn create(f: &Fixture) -> JevConnection {
    f.persistence
        .create_jev(
            f.target.company,
            f.target.actor,
            JevConnectionWrite {
                enabled: true,
                models: models("jev-1.14.0"),
                secret: SecretString::from("fixture-jev-secret"),
            },
        )
        .await
        .unwrap()
}
fn update(c: &JevConnection, enabled: bool, model: &str) -> JevConnectionUpdate {
    JevConnectionUpdate {
        expected_revision: c.revision,
        enabled,
        models: models(model),
    }
}
async fn assert_bound_envelope(
    p: &PostgresPersistence,
    c: &JevConnection,
    other_company: CompanyId,
) {
    let stored: String = sqlx::query_scalar(
        "SELECT envelope FROM company_typesafe_connections WHERE company_id=$1 AND id=$2",
    )
    .bind(c.company.as_uuid())
    .bind(c.id)
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert!(stored.starts_with("enc:v2:"));
    assert!(!stored.contains("fixture-jev-secret"));
    assert!(
        p.credential_cipher()
            .unwrap()
            .open_envelope(
                &CredentialContext::typesafe_credential(other_company.as_uuid(), c.id),
                &stored
            )
            .is_err()
    );
}

#[tokio::test]
async fn jev_connection_roundtrip_scope_revision_envelope_and_rotation() {
    let f = fixture().await;
    let other = Fixture::in_database(f._db.clone(), "other").await;
    let p = &f.persistence;
    let c = create(&f).await;
    assert!(c.ready);
    let public =
        serde_json::to_string(&p.list_jev(f.target.company, f.target.actor).await.unwrap())
            .unwrap();
    assert!(!public.contains("secret") && !public.contains("envelope"));
    assert!(
        p.read_jev(other.target.company, other.target.actor, c.id)
            .await
            .is_err()
    );
    assert!(
        p.read_jev(f.target.company, other.target.actor, c.id)
            .await
            .is_err()
    );
    assert!(
        p.rotate_jev(
            other.target.company,
            other.target.actor,
            c.id,
            1,
            SecretString::from("other-secret")
        )
        .await
        .is_err()
    );
    assert_bound_envelope(p, &c, other.target.company).await;
    let rotated = p
        .rotate_jev(
            c.company,
            f.target.actor,
            c.id,
            c.revision,
            SecretString::from("rotated-key"),
        )
        .await
        .unwrap();
    assert_eq!((rotated.id, rotated.revision), (c.id, 2));
    assert!(matches!(
        p.update_jev(
            c.company,
            f.target.actor,
            c.id,
            update(&c, false, "jev-1.14.0")
        )
        .await,
        Err(AppError::Conflict(_))
    ));
    let disabled = p
        .update_jev(
            c.company,
            f.target.actor,
            c.id,
            update(&rotated, false, "jev-1.13.0"),
        )
        .await
        .unwrap();
    let read = p.read_jev(c.company, f.target.actor, c.id).await.unwrap();
    assert_eq!(read.revision, disabled.revision);
    assert!(!read.enabled && !read.ready);
    assert_eq!(read.models, models("jev-1.13.0"));
    assert_eq!(
        p.inspect(c.company, f.target.actor, RuntimeResourceId::new(c.id))
            .await
            .unwrap()
            .unwrap()
            .readiness,
        crate::application::workflow::binding::ResourceReadiness::Revoked
    );
}

#[tokio::test]
async fn jev_connection_kek_rotation_inventory() {
    let f = fixture().await;
    let c = create(&f).await;
    let rotating = PostgresPersistence::with_credential_cipher(
        f.persistence.pool().clone(),
        CredentialCipher::for_test_with_keys(&[(1, [42; 32]), (2, [43; 32])], 2),
    );
    let before = rotating.credential_status().await.unwrap();
    assert_eq!(before.old_rows, 1);
    let report = rotating.rotate_credentials().await.unwrap();
    assert_eq!(report.rotated, 1);
    assert!(report.complete);
    let stored: String = sqlx::query_scalar(
        "SELECT envelope FROM company_typesafe_connections WHERE company_id=$1 AND id=$2",
    )
    .bind(c.company.as_uuid())
    .bind(c.id)
    .fetch_one(rotating.pool())
    .await
    .unwrap();
    let secret = rotating
        .credential_cipher()
        .unwrap()
        .open_envelope(
            &CredentialContext::typesafe_credential(c.company.as_uuid(), c.id),
            &stored,
        )
        .unwrap();
    assert_eq!(secret.expose_secret(), "fixture-jev-secret");
}

pub(super) async fn bound(f: &Fixture, c: &JevConnection) -> BindingTarget {
    let source: Value =
        serde_json::from_str(&registry::example("jev.evaluate").unwrap().source).unwrap();
    bound_source(f, c, source).await
}
pub(super) async fn bound_source(f: &Fixture, c: &JevConnection, source: Value) -> BindingTarget {
    let version = f.publish_workflow(f.target.workflow, source).await;
    let target = BindingTarget {
        company: f.target.company,
        actor: f.target.actor,
        binding: WorkflowBindingId::new(Uuid::new_v4()),
    };
    let p = &f.persistence;
    let service = BindingService::new(Preflight, p.clone(), p.clone(), p.clone());
    let state = service
        .configure(ConfigureBinding {
            target,
            expected: None,
            association: RelatedAssociation::Company,
            version,
            params: json!({}),
            resources: BTreeMap::from([(
                ResourceName::parse("service").unwrap(),
                RuntimeResourceId::new(c.id),
            )]),
            run_as: None,
            message_trigger: None,
        })
        .await
        .unwrap();
    service
        .activate(SetBindingActivity {
            target,
            expected: state.revision,
        })
        .await
        .unwrap();
    target
}
pub(super) async fn admit(
    f: &Fixture,
    target: BindingTarget,
    key: &str,
) -> AppResult<AdmissionResult> {
    let p = &f.persistence;
    WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight)
        .admit(AdmitWorkflowRequest {
            company_id: target.company,
            actor: target.actor,
            association: RelatedAssociation::Company,
            trigger: TriggerRef::new(
                target.company,
                TriggerId::new(Uuid::new_v4()),
                TriggerSource::Manual,
            )
            .unwrap(),
            correlation_id: CorrelationId::new(),
            binding_id: target.binding,
            idempotency_key: IdempotencyKey::parse(key).unwrap(),
            input: json!({}),
            rerun_of: None,
            delivery_policy: crate::application::workflow::DeliveryPolicy::Deliver,
            expected_binding_revision: None,
            admission_claim: None,
        })
        .await
}
pub(super) async fn claim(f: &Fixture, target: BindingTarget) -> ClaimedWorkflow {
    let AdmissionResult::Created(run) =
        admit(f, target, &format!("first.{}", target.binding.as_uuid()))
            .await
            .unwrap()
    else {
        panic!()
    };
    let (execution,job):(Uuid,Uuid) = sqlx::query_as("SELECT execution.id,job.id FROM workflow_executions AS execution JOIN background_tasks AS job ON job.workflow_execution_id=execution.id WHERE execution.run_id=$1").bind(run.as_uuid()).fetch_one(f.persistence.pool()).await.unwrap();
    f.persistence
        .claim_io(
            crate::application::workflow::activation::ActivationRequest {
                company: target.company,
                run,
                execution: ExecutionId::new(execution),
                job: crate::application::workflow::activation::WorkflowJobId(job),
            },
            WorkflowWorkerId(Uuid::new_v4()),
            LeasePolicy::new(Duration::from_secs(30)).unwrap(),
        )
        .await
        .unwrap()
        .unwrap()
}
#[tokio::test]
async fn jev_binding_current_rotation_model_removal_disable_and_no_fallback() {
    let f = fixture().await;
    let c = create(&f).await;
    let target = bound(&f, &c).await;
    let claim = claim(&f, target).await;
    let credential = f.persistence.authorize_jev(&claim).await.unwrap();
    assert_eq!(credential.connection_id, c.id);
    assert_eq!(credential.bearer.expose_secret(), "fixture-jev-secret");
    let c = f
        .persistence
        .rotate_jev(
            c.company,
            target.actor,
            c.id,
            c.revision,
            SecretString::from("new-secret"),
        )
        .await
        .unwrap();
    assert_eq!(
        f.persistence
            .authorize_jev(&claim)
            .await
            .unwrap()
            .bearer
            .expose_secret(),
        "new-secret"
    );
    let replacement = create(&f).await;
    assert_ne!(replacement.id, c.id);
    let c = f
        .persistence
        .update_jev(
            c.company,
            target.actor,
            c.id,
            update(&c, true, "jev-1.13.0"),
        )
        .await
        .unwrap();
    assert!(f.persistence.authorize_jev(&claim).await.is_err());
    assert!(admit(&f, target, "model-removed").await.is_err());
    let c = f
        .persistence
        .update_jev(
            c.company,
            target.actor,
            c.id,
            update(&c, false, "jev-1.14.0"),
        )
        .await
        .unwrap();
    assert!(admit(&f, target, "disabled").await.is_err());
    assert!(f.persistence.authorize_jev(&claim).await.is_err());
    sqlx::query("DELETE FROM company_typesafe_connections WHERE company_id=$1 AND id=$2")
        .bind(c.company.as_uuid())
        .bind(c.id)
        .execute(f.persistence.pool())
        .await
        .unwrap();
    assert!(f.persistence.authorize_jev(&claim).await.is_err());
    let snapshot:String=sqlx::query_scalar("SELECT jsonb_build_object('bundle',encode(bundle,'escape'),'resources',resources)::text FROM workflow_runs WHERE id=$1").bind(claim.fence.scope.run.as_uuid()).fetch_one(f.persistence.pool()).await.unwrap();
    assert!(!snapshot.contains("secret") && !snapshot.contains("enc:v2"));
}
#[tokio::test]
async fn jev_binding_competing_authorizations_observe_committed_revocation() {
    let f = fixture().await;
    let c = create(&f).await;
    let target = bound(&f, &c).await;
    let claim = claim(&f, target).await;
    let mut revoker = f.persistence.pool().begin().await.unwrap();
    authority::authorize_company(&mut revoker, c.company, target.actor)
        .await
        .unwrap();
    sqlx::query("UPDATE company_typesafe_connections SET enabled=false,revision=revision+1 WHERE company_id=$1 AND id=$2").bind(c.company.as_uuid()).bind(c.id).execute(&mut *revoker).await.unwrap();
    let (a, b, ()) = tokio::join!(
        f.persistence.authorize_jev(&claim),
        f.persistence.authorize_jev(&claim),
        async {
            crate::adapters::persistence::test_support::wait_until_backends_are_blocked(
                f.persistence.pool(),
                2,
            )
            .await;
            revoker.commit().await.unwrap();
        }
    );
    assert!(a.is_err() && b.is_err());
}
#[tokio::test]
async fn jev_binding_publication_requires_company_model_and_stale_fence_refused() {
    let f = fixture().await;
    let source = registry::example("jev.evaluate").unwrap().source;
    assert!(
        f.publish_source(
            &f.service(),
            f.target.workflow,
            source,
            VersionId::new(Uuid::new_v4())
        )
        .await
        .is_err()
    );
    let c = create(&f).await;
    // A different workflow avoids reusing the first draft's revision.
    let f = Fixture {
        target: DefinitionTarget {
            workflow: WorkflowId::new(Uuid::new_v4()),
            ..f.target
        },
        ..f
    };
    let target = bound(&f, &c).await;
    let mut claim = claim(&f, target).await;
    claim.fence.generation = WorkflowGeneration(Uuid::new_v4());
    assert!(f.persistence.authorize_jev(&claim).await.is_err());
}

#[tokio::test]
async fn jev_binding_foreign_selection_model_allowance_and_stale_revision() {
    let f = fixture().await;
    let c = create(&f).await;
    let target = bound(&f, &c).await;
    let p = &f.persistence;
    let state = p
        .binding(target.company, target.binding)
        .await
        .unwrap()
        .unwrap();
    let version = state
        .configuration
        .bundle()
        .compiled()
        .graph()
        .definition()
        .version_id;
    let service = BindingService::new(Preflight, p.clone(), p.clone(), p.clone());
    let request = |id, expected| ConfigureBinding {
        target,
        expected,
        association: RelatedAssociation::Company,
        version,
        params: json!({}),
        resources: BTreeMap::from([(
            ResourceName::parse("service").unwrap(),
            RuntimeResourceId::new(id),
        )]),
        run_as: None,
        message_trigger: None,
    };
    assert!(
        service
            .configure(request(c.id, Some(BindingStateRevision::new(1).unwrap())))
            .await
            .is_err()
    );
    let mut other = Fixture::in_database(f._db.clone(), "foreign").await;
    other.persistence =
        PostgresPersistence::with_credential_cipher(p.pool().clone(), CredentialCipher::for_test());
    let foreign = create(&other).await;
    assert!(
        service
            .configure(request(foreign.id, Some(state.revision)))
            .await
            .is_err()
    );
    let c = p
        .update_jev(
            c.company,
            target.actor,
            c.id,
            update(&c, true, "jev-1.13.0"),
        )
        .await
        .unwrap();
    assert!(
        service
            .configure(request(c.id, Some(state.revision)))
            .await
            .is_err()
    );
    assert!(
        service
            .activate(SetBindingActivity {
                target,
                expected: state.revision
            })
            .await
            .is_err()
    );
}

#[tokio::test]
async fn jev_binding_forged_inputs_and_current_actor_revocation_are_refused() {
    let f = fixture().await;
    let c = create(&f).await;
    let target = bound(&f, &c).await;
    let claim = claim(&f, target).await;
    let mut forged = claim.clone();
    forged.activation.inputs["model"] = json!("jev-1.13.0");
    assert!(f.persistence.authorize_jev(&forged).await.is_err());
    sqlx::query("DELETE FROM company_members WHERE company_id=$1 AND user_id=$2")
        .bind(target.company.as_uuid())
        .bind(target.actor.user_id())
        .execute(f.persistence.pool())
        .await
        .unwrap();
    assert!(f.persistence.authorize_jev(&claim).await.is_err());
}
