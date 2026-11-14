//! Actual storage collectors, retained source ownership and concurrent child propagation.
use super::*;
use crate::application::workflow::storage::{NotificationOriginCursor, WorkflowStorageMaintenance};

#[derive(sqlx::FromRow)]
struct Token {
    origin_id: Uuid,
    message_id: Uuid,
}

async fn tokens(f: &AgentCase) -> Vec<Token> {
    sqlx::query_as("SELECT notice_id AS origin_id,message_id FROM workflow_escalation_notice_targets WHERE company_id=$1 ORDER BY notice_id")
        .bind(f.people.role.company().as_uuid()).fetch_all(f.people.role.p().pool()).await.unwrap()
}

fn cursor(f: &AgentCase, token: &Token) -> NotificationOriginCursor {
    NotificationOriginCursor {
        company: f.people.role.company(),
        origin: token.origin_id,
    }
}

async fn deliver(f: &AgentCase) {
    let worker = notification_worker(f.people.role.p(), Default::default());
    for _ in 0..8 {
        if worker.drain_once().await.unwrap() == 0 {
            return;
        }
    }
    panic!("the finite genuine notification batch did not drain");
}

async fn source_binding(f: &AgentCase) {
    let binding = &f.people.role.admission.binding;
    let version = Box::pin(
        binding
            .fixture
            .publish_workflow(WorkflowId::new(Uuid::new_v4()), leaf(&json!(true))),
    )
    .await;
    let mut request = binding.request(Some(binding.state().await.revision));
    request.association = RelatedAssociation::Channel(RelatedChannelId::new(f.people.role.channel));
    request.version = version;
    request.params = json!({});
    request.resources = Default::default();
    request.escalation = Some(EscalationPolicy {
        notify: DepartmentRole::Reviewer,
        hops: EscalationHops::try_from(1).unwrap(),
    });
    binding.service().configure(request).await.unwrap();
}

async fn orphan_fixture() -> (AgentCase, Vec<Token>) {
    let (f, original) = Box::pin(pending_agent_from_run()).await;
    deliver(&f).await;
    Box::pin(source_binding(&f)).await;
    let mut sources = vec![original];
    for key in ["orphan-source-two", "orphan-source-three"] {
        let source = f.people.role.admitted(key).await;
        fail(&f, f.people.scope(source).await).await;
        sources.push(source);
    }
    deliver(&f).await;
    let tokens = tokens(&f).await;
    assert_eq!(
        tokens.len(),
        3,
        "all tokens came from actual run failures and fenced notice delivery"
    );
    for source in sources {
        retire(&f, source).await;
    }
    for token in &tokens[1..] {
        // Canonical-owner loss at the database boundary, after real source-tree
        // retirement. No origin/target guard is disabled or bypassed.
        assert_eq!(
            sqlx::query("DELETE FROM messages WHERE company_id=$1 AND id=$2")
                .bind(f.people.role.company().as_uuid())
                .bind(token.message_id)
                .execute(f.people.role.p().pool())
                .await
                .unwrap()
                .rows_affected(),
            1
        );
    }
    assert_eq!(counts(&f).await["messages"], 1);
    assert_eq!(counts(&f).await["origins"], 3);
    (f, tokens)
}

#[tokio::test]
async fn workflow_escalation_origin_real_cleanup_bounds_scanned_pages_skips_locked_prefix_and_collectors_delete_once()
 {
    let (f, tokens) = Box::pin(orphan_fixture()).await;
    let p = f.people.role.p();
    let first = p.sweep_notification_origins(None, 1).await.unwrap();
    assert_eq!(
        first.deleted, 0,
        "the retained canonical notice pins its token"
    );
    assert_eq!(first.next, Some(cursor(&f, &tokens[0])));
    let mut holder = p.pool().begin().await.unwrap();
    sqlx::query(
        "SELECT id FROM workflow_notification_origins WHERE company_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(f.people.role.company().as_uuid())
    .bind(tokens[1].origin_id)
    .execute(&mut *holder)
    .await
    .unwrap();
    let busy = p.sweep_notification_origins(first.next, 1).await.unwrap();
    assert_eq!(busy.deleted, 0);
    assert_eq!(
        busy.next,
        Some(cursor(&f, &tokens[1])),
        "a locked full page still advances discovery"
    );
    holder.rollback().await.unwrap();
    let (left, right) = tokio::join!(
        p.sweep_notification_origins(first.next, 1),
        p.sweep_notification_origins(first.next, 1)
    );
    let (left, right) = (left.unwrap(), right.unwrap());
    assert!(left.deleted <= 1 && right.deleted <= 1);
    let mut removed = left.deleted + right.deleted;
    let mut after = None;
    for _ in 0..6 {
        let swept = p.sweep_notification_origins(after, 1).await.unwrap();
        removed += swept.deleted;
        after = swept.next;
        if after.is_none() {
            break;
        }
    }
    assert!(
        after.is_none(),
        "finite scans reach exhaustion despite the retained prefix"
    );
    assert_eq!(
        removed, 2,
        "each genuine orphan is deleted exactly once across real collectors"
    );
    assert_eq!(counts(&f).await["origins"], 1);
    assert_eq!(counts(&f).await["messages"], 1);
    let refused =
        sqlx::query("DELETE FROM workflow_notification_origins WHERE company_id=$1 AND id=$2")
            .bind(f.people.role.company().as_uuid())
            .bind(tokens[0].origin_id)
            .execute(p.pool())
            .await
            .unwrap_err();
    assert_eq!(
        refused
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("23514")
    );
    for invalid in [0, 1025] {
        assert!(p.sweep_notification_origins(None, invalid).await.is_err());
    }
}

async fn blocked(p: &PostgresPersistence, holder: i32) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND $1=ANY(pg_blocking_pids(pid)))")
                .bind(holder).fetch_one(p.pool()).await.unwrap();
            if waiting { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("the real child insertion must wait on the held origin token");
}

#[tokio::test]
async fn workflow_escalation_origin_real_cleanup_preserves_token_during_actual_child_propagation_after_source_retirement()
 {
    let (f, source) = Box::pin(pending_agent_from_run()).await;
    Box::pin(configure_caller(&f)).await;
    deliver(&f).await;
    let mut claimed = claims(&f).await;
    assert_eq!(claimed.len(), 1);
    process(&f, claimed.pop().unwrap()).await;
    let parent = root(&f).await;
    let scope = active_scope(&f, parent).await;
    retire(&f, source).await;
    let tokens = tokens(&f).await;
    assert_eq!(tokens.len(), 1);
    let p = f.people.role.p();
    let mut holder = p.pool().begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *holder)
        .await
        .unwrap();
    sqlx::query(
        "SELECT id FROM workflow_notification_origins WHERE company_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(f.people.role.company().as_uuid())
    .bind(tokens[0].origin_id)
    .execute(&mut *holder)
    .await
    .unwrap();
    let collect = async {
        blocked(p, pid).await;
        let swept = p.sweep_notification_origins(None, 1).await.unwrap();
        assert_eq!(swept.deleted, 0);
        assert_eq!(swept.next, Some(cursor(&f, &tokens[0])));
        holder.rollback().await.unwrap();
    };
    let (child, ()) = tokio::join!(Box::pin(park(&f, scope)), collect);
    assert_origin(&f, child, tokens[0].origin_id).await;
    assert_eq!(
        p.sweep_notification_origins(None, 1).await.unwrap().deleted,
        0
    );
    assert_eq!(counts(&f).await["cases"], 0);
    assert_eq!(counts(&f).await["roots"], 1);
    assert_eq!(counts(&f).await["origins"], 1);
}
