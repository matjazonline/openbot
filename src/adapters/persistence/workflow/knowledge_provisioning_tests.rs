use super::*;
use crate::adapters::persistence::workflow::knowledge;
use crate::entities::access_level::AccessLevel;
#[tokio::test]
async fn knowledge_run_level_provisioning_rechecks_pending_intent_before_any_local_effect() {
    let (f, claim) = claimed(false).await;
    let request = call(&f, &claim, ProvisioningKind::ListAgents, json!({})).await;
    let before = written(&f).await;
    let scope = claim.fence.scope;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    knowledge::raise_content_level_on(&mut tx, scope.company, scope.run, AccessLevel::L5)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(matches!(
        dispatch(&f).dispatch_local(&request).await,
        Err(AppError::KnowledgeRelease {
            content: AccessLevel::L5,
            destination: AccessLevel::L0
        })
    ));
    assert_eq!(written(&f).await, before);
}
