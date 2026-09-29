use super::*;

async fn kind_tenant(f: &AdmissionFixture, kind: &str) -> AdmissionFixture {
    let mut fixture = Fixture::in_database(
        f.binding.fixture._db.clone(),
        &Uuid::new_v4().simple().to_string(),
    )
    .await;
    fixture.target.workflow = WorkflowId::new(Uuid::new_v4());
    let mut source: Value = serde_json::from_str(&registry::example(kind).unwrap().source).unwrap();
    source["workflow_id"] = json!(fixture.target.workflow.as_uuid());
    if kind == "wait.timer" {
        source["steps"]["start"]["with"]["deadline"] =
            json!({"literal":(chrono::Utc::now()+chrono::Duration::minutes(5)).to_rfc3339()});
    }
    AdmissionFixture::with_binding(BindingFixture::from_fixture(fixture, source).await).await
}

async fn until_run(p: &PostgresPersistence, scope: ActivationRequest, state: &str) {
    loop {
        let saved: String = sqlx::query_scalar("SELECT state FROM workflow_runs WHERE id=$1")
            .bind(scope.run.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
        if saved == state {
            return;
        }
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn workflow_fairness_real_loop_mixes_full_unsupported_poison_wait_hung_and_refill() {
    let (f, a) = fixture().await;
    let b = tenant(&f).await;
    let quiet = tenant(&f).await;
    let noise = tenant(&f).await;
    let unsupported = kind_tenant(&f, "memory.load").await;
    let waiting = kind_tenant(&f, "wait.timer").await;
    let p = f.persistence();
    p.configure_capacity(WorkflowCapacityPolicy::new(2, 1).unwrap())
        .await
        .unwrap();
    let bs = admit(&b, "hung-owner").await;
    // The base fixture binds max_tokens to /input/value; the generic tenant
    // example uses a literal and would accept the corrupted input below.
    let poison_scope = admit(&f, "poison").await;
    sqlx::query("UPDATE workflow_runs SET input='{}'::jsonb WHERE id=$1")
        .bind(poison_scope.run.as_uuid())
        .execute(p.pool())
        .await
        .unwrap();
    let wait_scope = admit(&waiting, "park").await;
    for i in 0..4 {
        admit(&unsupported, &format!("unsupported-{i}")).await;
    }
    let (started, mut starts) = mpsc::channel(8);
    let handler = Gated {
        gates: [a.company, bs.company, quiet.binding.target.company]
            .into_iter()
            .map(|c| (c.as_uuid(), Semaphore::new(0)))
            .collect(),
        started,
        live: AtomicUsize::new(0),
        maximum: AtomicUsize::new(0),
        unsupported_seen: AtomicUsize::new(0),
        companies: Mutex::new(HashMap::new()),
    };
    let aw = runner(p, &handler, worker());
    let bw = runner(p, &handler, worker());
    let qw = WorkflowWorker {
        poll: PollPolicy::new(Duration::from_millis(50), 2).unwrap(),
        ..runner(p, &handler, worker())
    };
    let cancel = CancellationToken::new();
    let owners_cancel = CancellationToken::new();
    let wakeup = tokio::sync::Notify::new();
    let loop_ready = Semaphore::new(0);
    let stage = AtomicUsize::new(0);
    tokio::time::timeout(Duration::from_secs(15),async {
        let control=async {
            let owners=[starts.recv().await.unwrap(),starts.recv().await.unwrap()];
            assert!(owners.contains(&a.company.as_uuid()) && owners.contains(&bs.company.as_uuid()));
            stage.store(1,Ordering::SeqCst);
            loop_ready.add_permits(1);
            until_run(p,poison_scope,"failed").await;
            stage.store(2,Ordering::SeqCst);
            until_run(p,wait_scope,"waiting").await;
            while handler.unsupported_seen.load(Ordering::SeqCst)<4 {tokio::task::yield_now().await;}
            stage.store(3,Ordering::SeqCst);
            let q=admit(&quiet,"quiet").await;
            loop {
                let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_dispatch_demand WHERE company_id=$1 AND worker_id=$2)")
                    .bind(q.company.as_uuid()).bind(qw.worker.0).fetch_one(p.pool()).await.unwrap();
                if waiting {break;}
                tokio::task::yield_now().await;
            }
            stage.store(4,Ordering::SeqCst);
            for i in 0..8 {denied(p,admit(&noise,&format!("before-{i}")).await,worker()).await;}
            handler.gates[&a.company.as_uuid()].add_permits(1);
            completed(p,a).await;
            assert_eq!(starts.recv().await.unwrap(),q.company.as_uuid());
            stage.store(5,Ordering::SeqCst);
            // The hung owner remains active throughout quiet progress and refill.
            assert_eq!(handler.live.load(Ordering::SeqCst),2);
            for i in 0..8 {denied(p,admit(&noise,&format!("during-{i}")).await,worker()).await;}
            let counts:Vec<(Uuid,i64)>=sqlx::query_as("SELECT company_id,count(*) FROM background_tasks WHERE status='processing' AND lock_expires_at>clock_timestamp() GROUP BY company_id")
                .fetch_all(p.pool()).await.unwrap();
            assert_eq!(counts.iter().map(|(_,count)|count).sum::<i64>(),2);
            assert!(counts.iter().all(|(_,count)|*count==1));
            // Quiet's actual future is still blocked; shutdown must drop it.
            cancel.cancel();
            handler.gates[&bs.company.as_uuid()].add_permits(1);
        };
        let polling=async {loop_ready.acquire().await.unwrap().forget();qw.run(&wakeup,&cancel).await;};
        let (ar,br,(),())=tokio::join!(aw.process(candidate(a),&owners_cancel),bw.process(candidate(bs),&owners_cancel),polling,control);
        assert!(ar.is_ok() && br.is_ok());
    }).await.unwrap_or_else(|_|panic!("mixed test stalled at stage {}",stage.load(Ordering::SeqCst)));
    assert_eq!(handler.maximum.load(Ordering::SeqCst), 2);
    assert_eq!(handler.live.load(Ordering::SeqCst), 0);
    let untouched: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_executions WHERE company_id=$1 AND frozen_inputs IS NULL",
    )
    .bind(unsupported.binding.target.company.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(untouched, 4);
    assert_eq!(budget(p, poison_scope).await["receipts"], Value::Null);
}
