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
}
impl Port {
    fn new(pending: bool) -> Self {
        Self {
            cursors: Mutex::new(Vec::new()),
            visits: AtomicUsize::new(0),
            pending,
            entered: Default::default(),
            dropped: AtomicBool::new(false),
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
    let mut cursor = None;
    sweep(&port, &monitor, &mut cursor).await;
    assert_eq!(cursor, Some(root(4)));
    sweep(&port, &monitor, &mut cursor).await;
    assert_eq!(cursor, None);
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
