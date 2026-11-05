//! Capture row-context diagnostics without installing a global subscriber.
use std::sync::{Arc, Mutex};
#[derive(Clone, Default)]
pub(super) struct LogCapture(Arc<Mutex<Vec<u8>>>);
impl LogCapture {
    pub(super) fn dispatch(&self) -> tracing::Dispatch {
        tracing::Dispatch::new(
            tracing_subscriber::fmt()
                .with_writer(self.clone())
                .with_ansi(false)
                .without_time()
                .finish(),
        )
    }
    pub(super) fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}
impl std::io::Write for LogCapture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogCapture {
    type Writer = Self;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}
