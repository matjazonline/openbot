use super::*;

/// The delegated creator can differ from the Builder's run owner. Deleting that
/// account must never invert account -> company locks during initial publication.
#[tokio::test]
async fn delegated_provisioning_locks_requester_before_company() {
    let d = Deployment::new(vec![
        LlmTurn::tool_call(
            CREATE_AGENT_CHANNEL,
            spec("locked-helper", ReplyMode::Autonomous),
        ),
        LlmTurn::text(r#"{"body":"Created helper."}"#),
    ])
    .await;
    let builder = builder(&d).await;
    let (admin, email) = person(&d, "admin").await;
    write(
        &d,
        &email,
        "builder",
        "Create a helper.",
        IngressOrigin::TrustedApplication,
    )
    .await;
    let mut account = d.p().pool().begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *account)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(admin)
        .execute(&mut *account)
        .await
        .unwrap();

    let (_, ordered) = tokio::join!(
        d.run_until(
            "the delegated Builder run finished",
            async || finished_runs(&d, builder).await == 1
        ),
        async {
            let ordered = company_unlocked_while_account_waits(&d, pid).await;
            account.rollback().await.unwrap();
            ordered
        },
    );
    assert!(
        ordered,
        "the worker must wait for the actual creator before holding the company"
    );
    let receipts = receipts(&d, builder).await;
    assert_eq!(receipts.len(), 1, "{receipts:?}");
    assert_eq!(
        receipts[0]["agent"]["slug"], "locked-helper",
        "{receipts:?}"
    );
    let actor: Uuid = sqlx::query_scalar(
        "SELECT actor_id FROM workflow_binding_revisions WHERE company_id=$1 AND binding_id=$2",
    )
    .bind(d.fixture.target.company.as_uuid())
    .bind(Uuid::parse_str(receipts[0]["binding_id"].as_str().unwrap()).unwrap())
    .fetch_one(d.p().pool())
    .await
    .unwrap();
    assert_eq!(actor, admin);
}

async fn company_unlocked_while_account_waits(d: &Deployment, holder: i32) -> bool {
    let blocked = tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let blocked: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)) AND query LIKE '%FROM users%')",
            ).bind(holder).fetch_one(d.p().pool()).await.unwrap();
            if blocked { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await;
    if blocked.is_err() {
        return false;
    }
    let mut company = d.p().pool().begin().await.unwrap();
    sqlx::query("SET LOCAL lock_timeout='500ms'")
        .execute(&mut *company)
        .await
        .unwrap();
    let result = sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(d.fixture.target.company.as_uuid())
        .execute(&mut *company)
        .await;
    company.rollback().await.unwrap();
    result.is_ok()
}
