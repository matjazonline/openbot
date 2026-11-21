//! The save boundary's thread-provenance branch against real admitted thread runs.
//! Tool dispatch, fencing and delivery are covered by memory_scope_proof_tests;
//! this test isolates current-thread visibility from directory visibility.
use super::super::super::admission_history_tests::Conversation;
use super::*;
use crate::application::workflow::lease::{WorkflowLeases, WorkflowWorkerId};

async fn participant(f: &AdmissionFixture, conversation: &Conversation) -> PrincipalId {
    let principal = PrincipalId::random();
    sqlx::query("INSERT INTO principals(company_id,id,kind,display_label) VALUES($1,$2,'external','Thread subject')")
        .bind(f.binding.target.company.as_uuid()).bind(principal.as_uuid()).execute(f.persistence().pool()).await.unwrap();
    sqlx::query(
        "INSERT INTO thread_principals(company_id,channel_id,thread_id,principal_id,role)
        VALUES($1,$2,$3,$4,'participant')",
    )
    .bind(f.binding.target.company.as_uuid())
    .bind(conversation.channel)
    .bind(conversation.thread)
    .bind(principal.as_uuid())
    .execute(f.persistence().pool())
    .await
    .unwrap();
    principal
}

#[tokio::test]
async fn memory_scope_tools_subject_visibility_accepts_current_thread_externals_and_refuses_other_threads()
 {
    let mut load: Value =
        serde_json::from_str(&registry::example("memory.load").unwrap().source).unwrap();
    load["steps"]["start"]["with"]["scope"] = json!({"literal":{"unit":"root"}});
    let f = AdmissionFixture::with_binding(BindingFixture::from_source(load).await).await;
    let current = Conversation::new(&f).await;
    let unrelated = Conversation::new(&f).await;
    let visible = participant(&f, &current).await;
    let hidden = participant(&f, &unrelated).await;
    let command = f
        .prepare(current.request(&f, "thread-subject", current.message))
        .await;
    f.persistence().admit(&command).await.unwrap();
    let scope = super::super::super::child_support::entry(&f, command.proposed_run_id()).await;
    let claim = f
        .persistence()
        .claim_io(
            scope,
            WorkflowWorkerId(Uuid::new_v4()),
            crate::adapters::persistence::workflow::tests::message_tests::support::lease(),
        )
        .await
        .unwrap()
        .unwrap();
    let mut db = f.persistence().pool().acquire().await.unwrap();
    use crate::adapters::persistence::workflow::checkpoints::memory_tools::subject_visible_on;
    assert!(
        subject_visible_on(&mut db, claim.fence, None)
            .await
            .unwrap()
    );
    assert!(
        subject_visible_on(&mut db, claim.fence, Some(visible))
            .await
            .unwrap()
    );
    assert!(
        !subject_visible_on(&mut db, claim.fence, Some(hidden))
            .await
            .unwrap()
    );
    sqlx::query(
        "DELETE FROM thread_principals WHERE company_id=$1 AND thread_id=$2 AND principal_id=$3",
    )
    .bind(f.binding.target.company.as_uuid())
    .bind(current.thread)
    .bind(visible.as_uuid())
    .execute(&mut *db)
    .await
    .unwrap();
    assert!(
        !subject_visible_on(&mut db, claim.fence, Some(visible))
            .await
            .unwrap()
    );
}
