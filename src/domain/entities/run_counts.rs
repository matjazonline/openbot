//! Open workflow runs grouped by visible channel (p08 D4.4).
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RunCounts {
    /// Queued runs.
    pub pending: u64,
    /// Running runs.
    pub active: u64,
    /// Runs waiting, for any reason.
    pub waiting: u64,
}

impl RunCounts {
    pub fn is_empty(&self) -> bool {
        self.pending == 0 && self.active == 0 && self.waiting == 0
    }

    fn add(&mut self, state: &str, count: u64) {
        match state {
            "queued" => self.pending += count,
            "running" => self.active += count,
            "waiting" => self.waiting += count,
            _ => {}
        }
    }
}

/// One `(channel, state)` group of open runs. A run without a channel counts toward the company
/// alone.
#[derive(Debug)]
pub struct RunCountRow {
    pub channel_id: Option<Uuid>,
    pub state: String,
    pub count: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RunCountSnapshot {
    pub company: RunCounts,
    pub channels: BTreeMap<Uuid, RunCounts>,
}

impl RunCountSnapshot {
    pub fn from_rows(rows: impl IntoIterator<Item = RunCountRow>) -> Self {
        let mut snapshot = Self::default();
        for row in rows.into_iter().filter(|row| row.count > 0) {
            snapshot.company.add(&row.state, row.count);
            if let Some(channel_id) = row.channel_id {
                snapshot
                    .channels
                    .entry(channel_id)
                    .or_default()
                    .add(&row.state, row.count);
            }
        }
        snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_open_runs_by_channel_and_company() {
        let channel = Uuid::new_v4();
        let row = |channel_id, state: &str, count| RunCountRow {
            channel_id,
            state: state.into(),
            count,
        };
        let snapshot = RunCountSnapshot::from_rows([
            row(Some(channel), "queued", 2),
            row(Some(channel), "running", 1),
            row(Some(channel), "waiting", 3),
            row(None, "queued", 4),
            row(Some(channel), "succeeded", 99),
        ]);
        assert_eq!(
            snapshot.company,
            RunCounts {
                pending: 6,
                active: 1,
                waiting: 3
            }
        );
        assert_eq!(
            snapshot.channels[&channel],
            RunCounts {
                pending: 2,
                active: 1,
                waiting: 3
            }
        );
        assert_eq!(
            snapshot.channels.len(),
            1,
            "a run without a channel is company-wide"
        );
        assert!(RunCountSnapshot::from_rows([]).company.is_empty());
    }
}
