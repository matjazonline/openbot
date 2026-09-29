use super::*;
use std::collections::HashMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::{Semaphore, mpsc};

#[path = "fairness_mixed_tests.rs"]
mod mixed_tests;

struct Gated {
    gates: HashMap<Uuid, Semaphore>,
    started: mpsc::Sender<Uuid>,
    live: AtomicUsize,
    maximum: AtomicUsize,
    unsupported_seen: AtomicUsize,
    companies: Mutex<HashMap<Uuid, usize>>,
}
struct Invocation<'a> {
    handler: &'a Gated,
    company: Uuid,
}
impl Drop for Invocation<'_> {
    fn drop(&mut self) {
        self.handler.live.fetch_sub(1, Ordering::SeqCst);
        *self
            .handler
            .companies
            .lock()
            .unwrap()
            .get_mut(&self.company)
            .unwrap() -= 1;
    }
}
#[async_trait]
impl WorkflowHandler for Gated {
    fn supports(&self, kind: &WorkflowStepKind) -> bool {
        let supported = kind.0 == "context.load";
        if !supported {
            self.unsupported_seen.fetch_add(1, Ordering::SeqCst);
        }
        supported
    }
    async fn execute(
        &self,
        _: &WorkflowStepKind,
        claim: &ClaimedWorkflow,
    ) -> WorkflowHandlerResult {
        let company = claim.fence.scope.company.as_uuid();
        let live = self.live.fetch_add(1, Ordering::SeqCst) + 1;
        self.maximum.fetch_max(live, Ordering::SeqCst);
        {
            let mut companies = self.companies.lock().unwrap();
            let count = companies.entry(company).or_default();
            *count += 1;
            assert!(*count <= 1, "per-company actual concurrency exceeded");
        }
        let _guard = Invocation {
            handler: self,
            company,
        };
        self.started.send(company).await.unwrap();
        self.gates[&company].acquire().await.unwrap().forget();
        Ok(json!({"items":[],"token_count":0}))
    }
}
fn runner<'a>(
    p: &'a PostgresPersistence,
    h: &'a Gated,
    id: WorkflowWorkerId,
) -> WorkflowWorker<'a, PostgresPersistence, Gated> {
    WorkflowWorker {
        port: p,
        handler: h,
        worker: id,
        lease: lease(),
        poll: PollPolicy::new(Duration::from_millis(50), 128).unwrap(),
    }
}
fn candidate(scope: ActivationRequest) -> PollCandidate {
    PollCandidate {
        scope,
        work: PollWork::Job,
    }
}
async fn completed(p: &PostgresPersistence, scope: ActivationRequest) {
    loop {
        let finished: bool = sqlx::query_scalar(
            "SELECT completed_at IS NOT NULL FROM workflow_executions WHERE id=$1",
        )
        .bind(scope.execution.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
        if finished {
            return;
        }
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn workflow_fairness_sustained_refill_dispatches_quiet_handlers_in_bounded_turns() {
    let (f, a) = fixture().await;
    let b = tenant(&f).await;
    let c = tenant(&f).await;
    let d = tenant(&f).await;
    let quiet = [tenant(&f).await, tenant(&f).await, tenant(&f).await];
    let p = f.persistence();
    p.configure_capacity(WorkflowCapacityPolicy::new(2, 1).unwrap())
        .await
        .unwrap();
    let bs = admit(&b, "held").await;
    let mut qs = vec![];
    for (i, q) in quiet.iter().enumerate() {
        qs.push(admit(q, &format!("quiet-{i}")).await);
    }
    let (started, mut starts) = mpsc::channel(16);
    let handler = Arc::new(Gated {
        gates: [
            a.company,
            bs.company,
            qs[0].company,
            qs[1].company,
            qs[2].company,
        ]
        .into_iter()
        .map(|company| (company.as_uuid(), Semaphore::new(0)))
        .collect(),
        started,
        live: AtomicUsize::new(0),
        maximum: AtomicUsize::new(0),
        unsupported_seen: AtomicUsize::new(0),
        companies: Mutex::new(HashMap::new()),
    });
    let aw = runner(p, &handler, worker());
    let bw = runner(p, &handler, worker());
    let qworkers = [worker(), worker(), worker()];
    let cancel = CancellationToken::new();
    tokio::time::timeout(Duration::from_secs(15),async {
        let controller=async {
            let initial=[starts.recv().await.unwrap(),starts.recv().await.unwrap()];
            assert!(initial.contains(&a.company.as_uuid()) && initial.contains(&bs.company.as_uuid()));
            for (scope,w) in qs.iter().zip(qworkers) {
                assert_eq!(runner(p,&handler,w).process(candidate(*scope),&cancel).await.unwrap(),WorkDisposition::Deferred);
            }
            sqlx::query("UPDATE workflow_dispatch_demand SET registered_at=clock_timestamp()-interval '2 minutes'")
                .execute(p.pool()).await.unwrap();
            for i in 0..9 {
                let noisy=[&f,&c,&d][i%3];
                denied(p,admit(noisy,&format!("full-refill-{i}")).await,worker()).await;
            }
            assert!(ticket(p,qs[0].company).await.1.is_none());
            // Full wait was longer than offer lifetime; a fresh poll still owns age.
            assert_eq!(p.poll_fair(qworkers[0],128).await.unwrap().candidates[0].scope,qs[0]);
            handler.gates[&a.company.as_uuid()].add_permits(1);
            completed(p,a).await;
            for (turn,(scope,w)) in qs.iter().zip(qworkers).enumerate() {
                for i in 0..6 {
                    let noisy=[&f,&c,&d][i%3];
                    denied(p,admit(noisy,&format!("release-{turn}-refill-{i}")).await,worker()).await;
                }
                let offered=ticket(p,scope.company).await;
                assert!(offered.1.is_some());
                let fresh=PostgresPersistence::new(p.pool().clone());
                let qw=runner(&fresh,&handler,w);
                let inspect=async {
                    assert_eq!(starts.recv().await.unwrap(),scope.company.as_uuid());
                    assert_eq!(handler.live.load(Ordering::SeqCst),2);
                    let counts:Vec<(Uuid,i64)>=sqlx::query_as("SELECT company_id,count(*) FROM background_tasks WHERE status='processing' AND lock_expires_at>clock_timestamp() GROUP BY company_id")
                        .fetch_all(p.pool()).await.unwrap();
                    assert_eq!(counts.iter().map(|(_,count)|count).sum::<i64>(),2);
                    assert!(counts.iter().all(|(_,count)|*count<=1));
                    // Noise remains replenished throughout every quiet dispatch.
                    for i in 0..3 {denied(p,admit(&c,&format!("during-{turn}-{i}")).await,worker()).await;}
                    handler.gates[&scope.company.as_uuid()].add_permits(1);
                };
                let (done,())=tokio::join!(qw.process(candidate(*scope),&cancel),inspect);
                assert_eq!(done.unwrap(),WorkDisposition::Done);
            }
            handler.gates[&bs.company.as_uuid()].add_permits(1);
        };
        let (a_result,b_result,())=tokio::join!(aw.process(candidate(a),&cancel),bw.process(candidate(bs),&cancel),controller);
        assert_eq!(a_result.unwrap(),WorkDisposition::Done);
        assert_eq!(b_result.unwrap(),WorkDisposition::Done);
    }).await.unwrap();
    assert_eq!(handler.maximum.load(Ordering::SeqCst), 2);
    assert_eq!(handler.live.load(Ordering::SeqCst), 0);
}

struct Selective {
    kind: WorkflowStepKind,
    calls: AtomicUsize,
}
#[async_trait]
impl WorkflowHandler for Selective {
    fn supports(&self, kind: &WorkflowStepKind) -> bool {
        kind == &self.kind
    }
    async fn execute(&self, kind: &WorkflowStepKind, _: &ClaimedWorkflow) -> WorkflowHandlerResult {
        assert_eq!(kind, &self.kind);
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(if kind.0 == "context.load" {
            json!({"items":[],"token_count":0})
        } else {
            json!({"items":[]})
        })
    }
}

#[tokio::test]
async fn workflow_fairness_complementary_workers_reach_actual_supported_dispatch() {
    let (f, a) = fixture().await;
    let mut memory = Fixture::in_database(
        f.binding.fixture._db.clone(),
        &Uuid::new_v4().simple().to_string(),
    )
    .await;
    memory.target.workflow = WorkflowId::new(Uuid::new_v4());
    let mut source: Value =
        serde_json::from_str(&registry::example("memory.load").unwrap().source).unwrap();
    source["workflow_id"] = json!(memory.target.workflow.as_uuid());
    let memory =
        AdmissionFixture::with_binding(BindingFixture::from_fixture(memory, source).await).await;
    let b = admit(&memory, "memory").await;
    let names = if a.company.as_uuid() < b.company.as_uuid() {
        ["memory.load", "context.load"]
    } else {
        ["context.load", "memory.load"]
    };
    let handlers = names.map(|name| Selective {
        kind: WorkflowStepKind(name.into()),
        calls: AtomicUsize::new(0),
    });
    let ids = [worker(), worker()];
    let cancel = CancellationToken::new();
    // With one shared company cursor this exact alternation gives BOTH workers
    // only unsupported work forever. Each capability now traverses independently.
    for _ in 0..4 {
        for index in 0..2 {
            let runner = WorkflowWorker {
                port: f.persistence(),
                handler: &handlers[index],
                worker: ids[index],
                lease: lease(),
                poll: PollPolicy::new(Duration::from_millis(50), 1).unwrap(),
            };
            for candidate in f
                .persistence()
                .poll_fair(ids[index], 1)
                .await
                .unwrap()
                .candidates
            {
                runner.process(candidate, &cancel).await.unwrap();
            }
        }
    }
    assert!(
        handlers
            .iter()
            .all(|handler| handler.calls.load(Ordering::SeqCst) == 1)
    );
}

#[tokio::test]
async fn workflow_fairness_worker_stops_full_page_at_first_denied_demand() {
    let (f, a) = fixture().await;
    let b = tenant(&f).await;
    let quiet = tenant(&f).await;
    let p = f.persistence();
    p.configure_capacity(WorkflowCapacityPolicy::new(2, 1).unwrap())
        .await
        .unwrap();
    p.claim_io(a, worker(), lease()).await.unwrap().unwrap();
    p.claim_io(admit(&b, "owner").await, worker(), lease())
        .await
        .unwrap()
        .unwrap();
    let q = admit(&quiet, "quiet-0").await;
    for i in 1..8 {
        admit(&quiet, &format!("quiet-{i}")).await;
    }
    let h = Selective {
        kind: WorkflowStepKind("context.load".into()),
        calls: AtomicUsize::new(0),
    };
    let w = worker();
    let runner = WorkflowWorker {
        port: p,
        handler: &h,
        worker: w,
        lease: lease(),
        poll: PollPolicy::new(Duration::from_millis(50), 128).unwrap(),
    };
    let cancel = CancellationToken::new();
    let wakeup = tokio::sync::Notify::new();
    let observe = async {
        let first = loop {
            let touched:Option<chrono::DateTime<chrono::Utc>>=sqlx::query_scalar("SELECT touched_at FROM workflow_poll_workers WHERE worker_id=$1 AND EXISTS(SELECT 1 FROM workflow_dispatch_demand WHERE worker_id=$1)")
                .bind(w.0).fetch_optional(p.pool()).await.unwrap();
            if let Some(touched) = touched {
                break touched;
            }
            tokio::task::yield_now().await;
        };
        loop {
            let touched: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
                "SELECT touched_at FROM workflow_poll_workers WHERE worker_id=$1",
            )
            .bind(w.0)
            .fetch_one(p.pool())
            .await
            .unwrap();
            if touched > first {
                break;
            }
            tokio::task::yield_now().await;
        }
        cancel.cancel();
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(runner.run(&wakeup, &cancel), observe);
    })
    .await
    .unwrap();
    let activated:i64=sqlx::query_scalar("SELECT count(*) FROM workflow_executions WHERE company_id=$1 AND frozen_inputs IS NOT NULL")
        .bind(q.company.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(activated, 1, "remainder of denied page must not activate");
    assert_eq!(h.calls.load(Ordering::SeqCst), 0);
}
