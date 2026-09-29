use super::*;

#[tokio::test]
async fn workflow_maintenance_worker_drains_full_unsupported_pages_without_dispatch() {
    let (f, scope) = fixture().await;
    for index in 0..4 {
        let command = f
            .prepare(f.request(&format!("worker-deadline-{index}"), f.manual()))
            .await;
        f.persistence().admit(&command).await.unwrap();
    }
    sqlx::query("UPDATE workflow_runs SET created_at=clock_timestamp()-interval '2 minutes',deadline=clock_timestamp()-interval '1 minute' WHERE company_id=$1")
        .bind(scope.company.as_uuid()).execute(f.persistence().pool()).await.unwrap();
    let handler = Scripted {
        calls: AtomicUsize::new(0),
        supported: false,
    };
    let worker = polling(f.persistence(), &handler);
    let wakeup = Notify::new();
    let cancel = CancellationToken::new();
    let finish = async {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let count: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM workflow_runs WHERE company_id=$1 AND state='failed'",
                )
                .bind(scope.company.as_uuid())
                .fetch_one(f.persistence().pool())
                .await
                .unwrap();
                if count == 5 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        cancel.cancel();
    };
    tokio::join!(worker.run(&wakeup, &cancel), finish);
    assert_eq!(handler.calls.load(Ordering::SeqCst), 0);
    let saved = snapshot(&f).await;
    assert_eq!(saved["attempts"], Value::Null);
    assert!(
        saved["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|job| job["status"] == "failed" && job["retry_count"] == 0)
    );
    for _ in 0..2 {
        assert!(
            f.persistence()
                .poll_work(None, 2)
                .await
                .unwrap()
                .candidates
                .is_empty()
        );
    }
    assert_eq!(snapshot(&f).await, saved);
}
