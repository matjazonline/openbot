//! Real admissions: current readers, immutable snapshots, and refusal without partial work.
use super::admission_history_tests::Conversation;
use super::*;
use crate::application::workflow::batch::WorkflowBatch;
use crate::entities::access_level::AccessLevel;

async fn levels(f: &AdmissionFixture, run: RunId) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('ceiling',knowledge_ceiling,'content',content_level) FROM workflow_runs WHERE company_id=$1 AND id=$2")
        .bind(f.binding.target.company.as_uuid()).bind(run.as_uuid()).fetch_one(f.persistence().pool()).await.unwrap()
}
async fn owner_party(f: &AdmissionFixture, c: &Conversation) {
    sqlx::query("INSERT INTO thread_principals(company_id,channel_id,thread_id,principal_id,role) SELECT $1,$2,$3,id,'participant' FROM principals WHERE company_id=$1 AND user_id=$4")
        .bind(f.binding.target.company.as_uuid()).bind(c.channel).bind(c.thread).bind(f.binding.target.actor.user_id()).execute(f.persistence().pool()).await.unwrap();
}
async fn outsider(f: &AdmissionFixture, c: &Conversation) {
    let principal = Uuid::new_v4();
    sqlx::query("INSERT INTO principals(id,company_id,kind,display_label) VALUES($1,$2,'external','outside')")
        .bind(principal).bind(f.binding.target.company.as_uuid()).execute(f.persistence().pool()).await.unwrap();
    sqlx::query("INSERT INTO thread_principals(company_id,channel_id,thread_id,principal_id,role) VALUES($1,$2,$3,$4,'participant')")
        .bind(f.binding.target.company.as_uuid()).bind(c.channel).bind(c.thread).bind(principal).execute(f.persistence().pool()).await.unwrap();
}
pub(super) async fn stamped(f: &AdmissionFixture, c: &Conversation, stamp: AccessLevel) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO messages(id,company_id,author_principal_id,subject,clean_text_body,direction,role,correlation_id,content_hash,audience,access_level,content_exact) SELECT $1,$2,id,'Private','classified input','outbound','system',$3,$4,'external_conversation',$5::smallint,true FROM principals WHERE company_id=$2 AND user_id=$6")
        .bind(id).bind(f.binding.target.company.as_uuid()).bind(Uuid::new_v4()).bind(vec![1_u8;32]).bind(stamp.as_i16()).bind(f.binding.target.actor.user_id()).execute(f.persistence().pool()).await.unwrap();
    sqlx::query("INSERT INTO thread_messages(id,company_id,channel_id,thread_id,message_id,entry_kind) VALUES($1,$2,$3,$4,$5,'conversation')")
        .bind(Uuid::new_v4()).bind(f.binding.target.company.as_uuid()).bind(c.channel).bind(c.thread).bind(id).execute(f.persistence().pool()).await.unwrap();
    id
}

#[tokio::test]
async fn knowledge_run_level_manual_and_replay_keep_admitted_ceiling() {
    let f = AdmissionFixture::new().await;
    let command = f.prepare(f.request("manual-level", f.manual())).await;
    f.persistence().admit(&command).await.unwrap();
    assert_eq!(
        levels(&f, command.proposed_run_id()).await,
        json!({"ceiling":9,"content":0})
    );
    assert_eq!(
        f.persistence().admit(&command).await.unwrap(),
        AdmissionResult::Replayed(command.proposed_run_id())
    );
    let mut tx = f.persistence().pool().begin().await.unwrap();
    assert_eq!(
        crate::adapters::persistence::workflow::knowledge::actor_level_on(
            &mut tx,
            command.company_id(),
            None
        )
        .await
        .unwrap(),
        AccessLevel::PUBLIC
    );
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn knowledge_run_level_message_empty_owner_and_mixed_cc_audiences() {
    let f = AdmissionFixture::new().await;
    struct AudienceCase {
        key: &'static str,
        owner: bool,
        mixed: bool,
        expected: i16,
    }
    for AudienceCase {
        key,
        owner,
        mixed,
        expected,
    } in [
        AudienceCase {
            key: "empty",
            owner: false,
            mixed: false,
            expected: 0,
        },
        AudienceCase {
            key: "owner",
            owner: true,
            mixed: false,
            expected: 9,
        },
        AudienceCase {
            key: "mixed",
            owner: true,
            mixed: true,
            expected: 0,
        },
    ] {
        let c = Conversation::new(&f).await;
        if owner {
            owner_party(&f, &c).await;
        }
        if mixed {
            outsider(&f, &c).await;
        }
        let command = f.prepare(c.request(&f, key, c.message)).await;
        f.persistence().admit(&command).await.unwrap();
        assert_eq!(
            levels(&f, command.proposed_run_id()).await,
            json!({"ceiling":expected,"content":0})
        );
    }
}

#[tokio::test]
async fn knowledge_run_level_stamped_trigger_taints_or_refuses_without_partial_admission() {
    let f = AdmissionFixture::new().await;
    let c = Conversation::new(&f).await;
    owner_party(&f, &c).await;
    let high = stamped(&f, &c, AccessLevel::L5).await;
    let command = f.prepare(c.request(&f, "stamped", high)).await;
    f.persistence().admit(&command).await.unwrap();
    assert_eq!(
        levels(&f, command.proposed_run_id()).await,
        json!({"ceiling":9,"content":5})
    );
    outsider(&f, &c).await;
    let next = stamped(&f, &c, AccessLevel::L5).await;
    let refused = f.prepare(c.request(&f, "above", next)).await;
    let before = f.counts().await;
    assert!(matches!(
        f.persistence().admit(&refused).await,
        Err(AppError::KnowledgeTrigger {
            content: AccessLevel::L5,
            ceiling: AccessLevel::L0
        })
    ));
    assert_eq!(f.counts().await, before);
}

#[tokio::test]
async fn knowledge_run_level_schema_rejects_ceiling_changes_lowering_and_out_of_range() {
    let f = AdmissionFixture::new().await;
    let command = f.prepare(f.request("guards", f.manual())).await;
    f.persistence().admit(&command).await.unwrap();
    let run = command.proposed_run_id();
    let mut tx = f.persistence().pool().begin().await.unwrap();
    crate::adapters::persistence::workflow::knowledge::raise_content_level_on(
        &mut tx,
        command.company_id(),
        run,
        AccessLevel::L5,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    for statement in [
        "UPDATE workflow_runs SET knowledge_ceiling=8 WHERE id=$1",
        "UPDATE workflow_runs SET content_level=4 WHERE id=$1",
        "UPDATE workflow_runs SET content_level=10 WHERE id=$1",
    ] {
        let error = sqlx::query(statement)
            .bind(run.as_uuid())
            .execute(f.persistence().pool())
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
    }
}

#[tokio::test]
async fn knowledge_run_level_child_inherits_and_competing_result_consumers_raise_parent_once() {
    use crate::application::workflow::waits::{WaitProgress, WorkflowWaits};
    let leaf: Value = serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
    let (f, parent) = AdmissionFixture::caller(leaf).await;
    let p = f.persistence();
    let mut tx = p.pool().begin().await.unwrap();
    crate::adapters::persistence::workflow::knowledge::raise_content_level_on(
        &mut tx,
        parent.company,
        parent.run,
        AccessLevel::L3,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let child = f.spawn(parent).await;
    assert_eq!(
        levels(&f, child.run).await,
        json!({"ceiling":9,"content":3})
    );
    let mut tx = p.pool().begin().await.unwrap();
    crate::adapters::persistence::workflow::knowledge::raise_content_level_on(
        &mut tx,
        child.company,
        child.run,
        AccessLevel::L5,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    p.advance_pure(
        child,
        crate::application::workflow::batch::BatchBudget::new(4, std::time::Duration::from_secs(5))
            .unwrap(),
    )
    .await
    .unwrap();
    let (first, second) = tokio::join!(p.resume_wait(parent), p.resume_wait(parent));
    assert!(matches!(first.unwrap(), WaitProgress::Completed(_)));
    assert!(matches!(second.unwrap(), WaitProgress::Completed(_)));
    assert_eq!(
        levels(&f, parent.run).await,
        json!({"ceiling":9,"content":5})
    );
    let error =
        sqlx::query("UPDATE workflow_runs SET content_level=6 WHERE company_id=$1 AND id=$2")
            .bind(child.company.as_uuid())
            .bind(child.run.as_uuid())
            .execute(p.pool())
            .await
            .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("workflow_guard:workflow_run_knowledge")
    );
}

#[tokio::test]
async fn knowledge_run_level_manual_uses_actual_admin_starter_and_keeps_member_refused() {
    let f = AdmissionFixture::new().await;
    for role in ["admin", "member"] {
        let actor = super::super::message_tests::support::member(&f.binding.fixture, role).await;
        sqlx::query("UPDATE company_members SET access_level=5 WHERE company_id=$1 AND user_id=$2")
            .bind(f.binding.target.company.as_uuid())
            .bind(actor)
            .execute(f.persistence().pool())
            .await
            .unwrap();
        let mut request = f.request(role, f.manual());
        request.actor = WorkflowActor::authenticated(actor).unwrap();
        let command = f.prepare(request).await;
        if role == "admin" {
            f.persistence().admit(&command).await.unwrap();
            assert_eq!(
                levels(&f, command.proposed_run_id()).await,
                json!({"ceiling":5,"content":0})
            );
        } else {
            assert!(matches!(
                f.persistence().admit(&command).await,
                Err(AppError::NotFound(_))
            ));
        }
    }
}

/// Exactness only falls, never changes after the run ends, and a message's bit has the
/// role's shape (phase 05 E3).
#[tokio::test]
async fn memory_access_level_schema_only_lowers_exactness_and_shapes_message_bits() {
    use crate::adapters::persistence::workflow::knowledge::{
        exactness_fixture::{exact, mark_exact},
        lower_content_exact_on,
    };
    let f = AdmissionFixture::new().await;
    let command = f.prepare(f.request("exact-guards", f.manual())).await;
    f.persistence().admit(&command).await.unwrap();
    let (company, run) = (command.company_id(), command.proposed_run_id());
    let pool = f.persistence().pool();
    mark_exact(pool, company, run).await;
    let mut tx = pool.begin().await.unwrap();
    lower_content_exact_on(&mut tx, company, run).await.unwrap();
    lower_content_exact_on(&mut tx, company, run).await.unwrap();
    tx.commit().await.unwrap();
    assert!(!exact(pool, company, run).await);
    let raised = sqlx::query("UPDATE workflow_runs SET content_exact=true WHERE id=$1")
        .bind(run.as_uuid())
        .execute(pool)
        .await
        .unwrap_err();
    assert_eq!(
        raised.as_database_error().unwrap().constraint(),
        Some("workflow_guard:workflow_run_knowledge")
    );
    let c = Conversation::new(&f).await;
    for (role, bit) in [("human", Some(true)), ("agent", None), ("system", None)] {
        let error = sqlx::query("INSERT INTO messages(id,company_id,author_principal_id,subject,clean_text_body,direction,role,correlation_id,content_hash,audience,content_exact) SELECT $1,$2,id,'Shape','text','outbound',$3,$4,$5,'external_conversation',$6 FROM principals WHERE company_id=$2 AND user_id=$7")
            .bind(Uuid::new_v4()).bind(company.as_uuid()).bind(role).bind(Uuid::new_v4()).bind(vec![3_u8;32]).bind(bit)
            .bind(f.binding.target.actor.user_id()).execute(pool).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().constraint(),
            Some("messages_content_exact_shape"),
            "{role}"
        );
    }
    let changed = sqlx::query("UPDATE messages SET content_exact=true WHERE id=$1")
        .bind(c.message)
        .execute(pool)
        .await
        .unwrap_err();
    assert_eq!(
        changed.as_database_error().unwrap().constraint(),
        Some("workflow_guard:message_access_level")
    );
}
