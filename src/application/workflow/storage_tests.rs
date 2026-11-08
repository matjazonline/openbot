use super::*;
use crate::adapters::monitoring::InMemoryMonitor;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use uuid::Uuid;

struct Port {
    cursors: Mutex<Vec<Option<RetentionRoot>>>,
    visits: AtomicUsize,
    pending: bool,
    entered: tokio::sync::Notify,
    dropped: AtomicBool,
    history_records: Vec<HistoryCandidate>,
    history_cursors: Mutex<Vec<Option<HistoryCandidate>>>,
    history_visits: Mutex<Vec<Uuid>>,
    scope_companies: Vec<CompanyId>,
    scope_cursors: Mutex<Vec<Option<CompanyId>>>,
    scope_limits: Mutex<Vec<u16>>,
}
impl Port {
    fn new(pending: bool) -> Self {
        Self {
            cursors: Mutex::new(Vec::new()),
            visits: AtomicUsize::new(0),
            pending,
            entered: Default::default(),
            dropped: AtomicBool::new(false),
            history_records: vec![],
            history_cursors: Mutex::new(vec![]),
            history_visits: Mutex::new(vec![]),
            scope_companies: vec![],
            scope_cursors: Mutex::new(vec![]),
            scope_limits: Mutex::new(vec![]),
        }
    }
}
fn root(id: u128) -> RetentionRoot {
    RetentionRoot {
        finished_at: DateTime::from_timestamp(100, 0).unwrap(),
        company: CompanyId::new(Uuid::from_u128(1)),
        run: RunId::new(Uuid::from_u128(id)),
    }
}
struct PendingWrite<'a>(&'a AtomicBool);
impl Drop for PendingWrite<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}
#[async_trait]
impl WorkflowStorageMaintenance for Port {
    async fn sweep_obsolete_witnesses(&self, _: u16) -> AppResult<u64> {
        Ok(0)
    }
    async fn history_page(&self, after: Option<HistoryCandidate>) -> AppResult<HistoryPage> {
        self.history_cursors.lock().unwrap().push(after);
        Ok(HistoryPage {
            candidates: self
                .history_records
                .iter()
                .filter(|candidate| {
                    after
                        .is_none_or(|after| candidate.exchange.as_uuid() > after.exchange.as_uuid())
                })
                .take(usize::from(HISTORY_EXPIRY_SWEEP_LIMIT))
                .copied()
                .collect(),
        })
    }
    async fn expire_history(&self, candidate: HistoryCandidate) -> AppResult<bool> {
        self.history_visits
            .lock()
            .unwrap()
            .push(candidate.exchange.as_uuid());
        match candidate.exchange.as_uuid().as_u128() {
            1 => Err(AppError::DatabaseTimeout("busy history record".into())),
            2..=128 => Ok(false),
            _ => Ok(true),
        }
    }
    async fn history_scope_page(&self, after: Option<CompanyId>) -> AppResult<Vec<CompanyId>> {
        self.scope_cursors.lock().unwrap().push(after);
        Ok(self
            .scope_companies
            .iter()
            .filter(|company| after.is_none_or(|after| company.as_uuid() > after.as_uuid()))
            .take(RETENTION_PAGE_SIZE)
            .copied()
            .collect())
    }
    async fn prune_history(&self, company: CompanyId, limit: u16) -> AppResult<u64> {
        self.scope_limits.lock().unwrap().push(limit);
        if company.as_uuid().as_u128() == 1 {
            return Err(AppError::DatabaseTimeout("busy history scope".into()));
        }
        Ok(1)
    }
    async fn retention_page(&self, after: Option<RetentionRoot>) -> AppResult<RetentionPage> {
        self.cursors.lock().unwrap().push(after);
        Ok(if after.is_none() {
            RetentionPage {
                roots: (1..=4).map(root).collect(),
                next: Some(root(4)),
            }
        } else {
            RetentionPage {
                roots: vec![root(5)],
                next: None,
            }
        })
    }
    async fn retire_tree(&self, root: RetentionRoot) -> AppResult<Option<RetiredTree>> {
        self.visits.fetch_add(1, Ordering::SeqCst);
        if self.pending {
            let _write = PendingWrite(&self.dropped);
            self.entered.notify_one();
            return std::future::pending().await;
        }
        match root.run.as_uuid().as_u128() {
            1 => Err(AppError::Timeout("protected lock".into())),
            2..=4 => Ok(None),
            _ => Ok(Some(RetiredTree {
                runs: 2,
                jobs: 2,
                calls: 1,
            })),
        }
    }
    async fn sweep_unused_bundles(&self, _: u16) -> AppResult<u64> {
        Ok(0)
    }
    async fn oldest_terminal_root(&self) -> AppResult<Option<DateTime<Utc>>> {
        self.entered.notify_one();
        Ok(None)
    }
}

#[tokio::test]
async fn workflow_storage_cursor_advances_past_full_error_and_protected_page_without_time_advance()
{
    let port = Port::new(false);
    let monitor = InMemoryMonitor::new();
    let mut cursor = SweepCursor::default();
    sweep(&port, &monitor, &mut cursor).await;
    assert_eq!(cursor.root, Some(root(4)));
    sweep(&port, &monitor, &mut cursor).await;
    assert_eq!(cursor.root, None);
    assert_eq!(*port.cursors.lock().unwrap(), vec![None, Some(root(4))]);
    assert_eq!(port.visits.load(Ordering::SeqCst), 5);
}
#[tokio::test]
async fn workflow_storage_shutdown_drops_pending_write_and_joins() {
    let port = Port::new(true);
    let monitor = InMemoryMonitor::new();
    let shutdown = CancellationToken::new();
    let stop = async {
        port.entered.notified().await;
        shutdown.cancel();
    };
    tokio::time::timeout(Duration::from_secs(1), async {
        tokio::join!(run(&port, &monitor, &shutdown), stop);
    })
    .await
    .unwrap();
    assert!(port.dropped.load(Ordering::SeqCst));
    assert_eq!(port.visits.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn workflow_storage_full_error_page_still_has_positive_worker_delay() {
    let port = Port::new(false);
    let monitor = InMemoryMonitor::new();
    let shutdown = CancellationToken::new();
    let stop = async {
        port.entered.notified().await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(port.cursors.lock().unwrap().len(), 1);
        shutdown.cancel();
    };
    tokio::time::timeout(Duration::from_secs(1), async {
        tokio::join!(run(&port, &monitor, &shutdown), stop);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn workflow_storage_history_cursors_advance_past_error_and_protected_prefix_and_share_the_record_budget()
 {
    let mut port = Port::new(false);
    port.history_records = (1..=129)
        .map(|id| HistoryCandidate {
            company: CompanyId::new(Uuid::from_u128(1)),
            finished_at: DateTime::from_timestamp(100, 0).unwrap(),
            exchange: crate::application::workflow::agent_history::AgentExchangeId::new(
                Uuid::from_u128(id),
            ),
        })
        .collect();
    port.scope_companies = (1..=5)
        .map(|id| CompanyId::new(Uuid::from_u128(id)))
        .collect();
    let monitor = InMemoryMonitor::new();
    let mut cursor = SweepCursor::default();
    sweep(&port, &monitor, &mut cursor).await;
    assert_eq!(cursor.history, Some(port.history_records[127]));
    assert_eq!(cursor.history_scope, Some(port.scope_companies[3]));
    assert_eq!(port.history_visits.lock().unwrap().len(), 128);
    assert_eq!(
        port.scope_limits
            .lock()
            .unwrap()
            .iter()
            .copied()
            .sum::<u16>()
            + HISTORY_EXPIRY_SWEEP_LIMIT,
        STORAGE_SWEEP_LIMIT
    );
    sweep(&port, &monitor, &mut cursor).await;
    assert_eq!(cursor.history, Some(port.history_records[128]));
    assert_eq!(cursor.history_scope, Some(port.scope_companies[4]));
    sweep(&port, &monitor, &mut cursor).await;
    assert!(cursor.history.is_none() && cursor.history_scope.is_none());
    assert_eq!(
        *port.history_cursors.lock().unwrap(),
        vec![
            None,
            Some(port.history_records[127]),
            Some(port.history_records[128])
        ]
    );
    assert_eq!(
        *port.scope_cursors.lock().unwrap(),
        vec![
            None,
            Some(port.scope_companies[3]),
            Some(port.scope_companies[4])
        ]
    );
    assert_eq!(
        *port.history_visits.lock().unwrap(),
        (1..=129).map(Uuid::from_u128).collect::<Vec<_>>()
    );
}
