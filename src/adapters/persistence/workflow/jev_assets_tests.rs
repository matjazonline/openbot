use super::jev_runtime_tests::expire_reclaim;
use super::jev_tests::{bound_source, claim, create, fixture};
use super::*;
use crate::adapters::{
    storage::{BucketKind, FileStorage, test_support::FakeStorage},
    workflow_jev::{JevAssetReader, tests::server},
};
use crate::application::{
    use_cases::channel::{ChannelPersistence, ChannelWrite},
    workflow::{
        jev::assets::tests as images, jev::*, lease::*, polling::WorkflowStepKind,
        worker::WorkflowHandler,
    },
};
use crate::domain::entities::value_objects::ObjectKey;
use serde_json::{Value, json};
use std::time::Duration;
fn policy() -> LeasePolicy {
    LeasePolicy::new(Duration::from_secs(30)).unwrap()
}
pub(super) struct Attachment {
    pub(super) reference: Value,
    pub(super) key: ObjectKey,
}
pub(super) async fn attachment(f: &Fixture, bytes: &[u8]) -> Attachment {
    let p = &f.persistence;
    let company = f.target.company.as_uuid();
    let channel = ChannelPersistence::create(
        p,
        company,
        ChannelWrite {
            name: "Images".into(),
            slug: format!("images-{}", Uuid::new_v4()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let thread = Uuid::new_v4();
    let message = Uuid::new_v4();
    let key = ObjectKey::new(format!("attachments/{company}/{}.png", Uuid::new_v4()));
    sqlx::query("INSERT INTO threads(id,company_id,channel_id,subject) VALUES($1,$2,$3,'Images')")
        .bind(thread)
        .bind(company)
        .bind(channel.id)
        .execute(p.pool())
        .await
        .unwrap();
    let metadata = json!({"version":"1","items":[{"filename":"fixture.png","content_type":"image/png","sha256_hash":ImageDigest::of(bytes),"size_bytes":bytes.len(),"storage_key":key.as_str()}]});
    sqlx::query("INSERT INTO messages(id,company_id,author_principal_id,subject,clean_text_body,direction,role,correlation_id,content_hash,attachments) SELECT $1,$2,principal.id,'Images','body','inbound','human',$3,$4,$5 FROM principals AS principal WHERE principal.company_id=$2 AND principal.user_id=$6")
        .bind(message).bind(company).bind(Uuid::new_v4()).bind(vec![0u8;32]).bind(metadata).bind(f.target.actor.user_id()).execute(p.pool()).await.unwrap();
    sqlx::query("INSERT INTO thread_messages(id,company_id,channel_id,thread_id,message_id,entry_kind) VALUES($1,$2,$3,$4,$5,'conversation')").bind(Uuid::new_v4()).bind(company).bind(channel.id).bind(thread).bind(message).execute(p.pool()).await.unwrap();
    Attachment {
        reference: json!({"channel_id":channel.id,"thread_id":thread,"sha256":ImageDigest::of(bytes)}),
        key,
    }
}
async fn image_claim(f: &Fixture, attachment: &Attachment) -> ClaimedWorkflow {
    let c = create(f).await;
    let mut source: Value =
        serde_json::from_str(&registry::example("jev.evaluate").unwrap().source).unwrap();
    source["steps"]["start"]["with"]["questions"] =
        json!({"literal":images::inputs(b"inert")["questions"]});
    source["steps"]["start"]["with"]["assets"] = json!({"literal":{"photo":attachment.reference}});
    let target = bound_source(f, &c, source).await;
    claim(f, target).await
}
async fn calls(p: &PostgresPersistence, claim: &ClaimedWorkflow) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM workflow_jev_invocations WHERE company_id=$1 AND run_id=$2",
    )
    .bind(claim.fence.scope.company.as_uuid())
    .bind(claim.fence.scope.run.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap()
}
#[tokio::test]
async fn jev_assets_cross_company_negative_and_forged_claim_are_denied_before_storage() {
    let f = fixture().await;
    let other = Fixture::in_database(f._db.clone(), "foreign").await;
    let bytes = images::png(280, 280);
    let foreign = attachment(&other, &bytes).await;
    let claim = image_claim(&f, &foreign).await;
    // A storage failure would be retryable: authorization must reject before touching it.
    let reader = JevAssetReader::new(
        f.persistence.clone(),
        Some(Arc::new(FakeStorage::failing())),
    );
    assert!(matches!(
        reader.read(&claim).await,
        Err(JevAssetError::Unauthorized)
    ));
    assert_eq!(calls(&f.persistence, &claim).await, 0);
    let local = attachment(&f, &bytes).await;
    let mut forged = claim.clone();
    forged.activation.inputs["assets"]["photo"] = local.reference;
    assert!(matches!(
        reader.read(&forged).await,
        Err(JevAssetError::Unauthorized)
    ));
}
#[tokio::test]
async fn jev_assets_changed_object_retry_never_sends_or_adds_reservation() {
    let f = fixture().await;
    let bytes = images::png(280, 280);
    let attachment = attachment(&f, &bytes).await;
    let first = image_claim(&f, &attachment).await;
    let storage = Arc::new(FakeStorage::new());
    storage
        .store_object(
            BucketKind::Private,
            &attachment.key,
            "image/png",
            bytes.clone(),
        )
        .await
        .unwrap();
    let reader = Arc::new(JevAssetReader::new(
        f.persistence.clone(),
        Some(storage.clone()),
    ));
    let (http, task) = server(503, "", vec![], Duration::ZERO).await;
    let runtime = JevRuntime::new(f.persistence.clone(), Arc::new(http), policy())
        .with_assets(reader.clone());
    assert!(matches!(
        runtime
            .execute(&WorkflowStepKind("jev.evaluate".into()), &first)
            .await,
        WorkflowHandlerOutcome::Completed(Err(_))
    ));
    task.await.unwrap();
    assert_eq!(calls(&f.persistence, &first).await, 1);
    let second = expire_reclaim(&f.persistence, &first).await;
    let mut changed = bytes;
    changed[20] ^= 1;
    storage
        .store_object(BucketKind::Private, &attachment.key, "image/png", changed)
        .await
        .unwrap();
    let (http, task) = server(
        200,
        "",
        serde_json::to_vec(&images::output()).unwrap(),
        Duration::ZERO,
    )
    .await;
    let runtime =
        JevRuntime::new(f.persistence.clone(), Arc::new(http), policy()).with_assets(reader);
    let result = runtime
        .execute(&WorkflowStepKind("jev.evaluate".into()), &second)
        .await;
    let WorkflowHandlerOutcome::Completed(Err(failure)) = result else {
        panic!("changed image must fail")
    };
    assert_eq!(failure.failure.code().as_str(), "jev.asset_changed");
    assert_eq!(calls(&f.persistence, &second).await, 1);
    assert!(!task.is_finished());
    task.abort();
    let _ = task.await;
}
#[tokio::test]
async fn jev_assets_mixed_loopback_settlement_replay_and_metadata_only_ledger() {
    let f = fixture().await;
    let bytes = images::png(280, 280);
    let attachment = attachment(&f, &bytes).await;
    let first = image_claim(&f, &attachment).await;
    let storage = Arc::new(FakeStorage::new());
    storage
        .store_object(BucketKind::Private, &attachment.key, "image/png", bytes)
        .await
        .unwrap();
    let reader = Arc::new(JevAssetReader::new(f.persistence.clone(), Some(storage)));
    let (http, task) = server(
        200,
        "",
        serde_json::to_vec(&images::output()).unwrap(),
        Duration::ZERO,
    )
    .await;
    let runtime =
        JevRuntime::new(f.persistence.clone(), Arc::new(http), policy()).with_assets(reader);
    let result = runtime
        .execute(&WorkflowStepKind("jev.evaluate".into()), &first)
        .await;
    let WorkflowHandlerOutcome::Completed(Ok(value)) = result else {
        panic!("image completion expected")
    };
    assert_eq!(value, images::output());
    let sent = task.await.unwrap();
    assert!(sent.contains("typesafe-danger-zone-images: true"));
    let prepared: Vec<u8> = sqlx::query_scalar(
        "SELECT prepared FROM workflow_jev_invocations WHERE company_id=$1 AND execution_id=$2",
    )
    .bind(first.fence.scope.company.as_uuid())
    .bind(first.fence.scope.execution.as_uuid())
    .fetch_one(f.persistence.pool())
    .await
    .unwrap();
    assert!(!String::from_utf8_lossy(&prepared).contains("data_base64"));
    assert!(String::from_utf8_lossy(&prepared).contains("sha256"));
    let second = expire_reclaim(&f.persistence, &first).await;
    assert!(matches!(
        runtime
            .execute(&WorkflowStepKind("jev.evaluate".into()), &second)
            .await,
        WorkflowHandlerOutcome::Completed(Ok(_))
    ));
    assert_eq!(calls(&f.persistence, &second).await, 1);
}

#[tokio::test]
async fn jev_assets_current_actor_channel_visibility_and_missing_file() {
    let f = fixture().await;
    let bytes = images::png(280, 280);
    let attachment = attachment(&f, &bytes).await;
    let claim = image_claim(&f, &attachment).await;
    let reader = JevAssetReader::new(f.persistence.clone(), Some(Arc::new(FakeStorage::new())));
    assert!(matches!(
        reader.read(&claim).await,
        Err(JevAssetError::Missing)
    ));
    let other = Fixture::in_database(f._db.clone(), "newowner").await;
    sqlx::query("UPDATE companies SET user_id=$2 WHERE id=$1")
        .bind(f.target.company.as_uuid())
        .bind(other.target.actor.user_id())
        .execute(f.persistence.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE company_members SET role='admin' WHERE company_id=$1 AND user_id=$2")
        .bind(f.target.company.as_uuid())
        .bind(f.target.actor.user_id())
        .execute(f.persistence.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE channels SET access_mode='allowlist' WHERE company_id=$1 AND id=$2")
        .bind(f.target.company.as_uuid())
        .bind(
            attachment.reference["channel_id"]
                .as_str()
                .unwrap()
                .parse::<Uuid>()
                .unwrap(),
        )
        .execute(f.persistence.pool())
        .await
        .unwrap();
    assert!(matches!(
        reader.read(&claim).await,
        Err(JevAssetError::Unauthorized)
    ));
    assert_eq!(calls(&f.persistence, &claim).await, 0);
}

#[path = "jev_assets_race_tests.rs"]
mod race;
