//! Per-connection, synchronous SDK handoff pause; never introduces a production await.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, Sender},
};
use tokio::sync::Notify;

pub(super) struct RegistrationProbe {
    pub enqueued: Notify,
    pub delete_entered: Notify,
    pub cleanup_claimed: Notify,
    pub gate_blocked: AtomicBool,
    release: Mutex<Receiver<()>>,
}

impl RegistrationProbe {
    pub fn new() -> (Arc<Self>, Sender<()>) {
        let (release, receiver) = std::sync::mpsc::channel();
        (
            Arc::new(Self {
                enqueued: Notify::new(),
                delete_entered: Notify::new(),
                cleanup_claimed: Notify::new(),
                gate_blocked: AtomicBool::new(false),
                release: Mutex::new(receiver),
            }),
            release,
        )
    }

    pub fn pause(&self) {
        // Let other SDK tasks use this worker's Tokio core while this poll stays synchronous.
        tokio::task::block_in_place(|| {
            self.enqueued.notify_one();
            // Sender drop also releases a failed test; no blocked thread survives an assertion.
            self.release
                .lock()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(3))
                .expect("test must release the SDK handoff within the existing cancellation bound");
        });
    }

    pub fn deleting(&self, blocked: bool) {
        self.gate_blocked.store(blocked, Ordering::SeqCst);
        self.delete_entered.notify_one();
    }
}
