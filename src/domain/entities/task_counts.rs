//! Open work grouped by visible channel and current agent owner.
use super::task::TaskStatus;
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TaskCounts {
    pub pending: u64,
    pub active: u64,
    pub waiting: u64,
}

impl TaskCounts {
    pub fn is_empty(&self) -> bool {
        self.pending == 0 && self.active == 0 && self.waiting == 0
    }

    fn add(&mut self, status: TaskStatus, count: u64) {
        match status {
            TaskStatus::Pending => self.pending += count,
            TaskStatus::Processing => self.active += count,
            TaskStatus::PendingApproval | TaskStatus::WaitingForThirdPartyReply => {
                self.waiting += count
            }
            TaskStatus::Completed
            | TaskStatus::Failed
            | TaskStatus::DeadLetter
            | TaskStatus::Stopped => {}
        }
    }
}

#[derive(Debug)]
pub struct TaskCountRow {
    pub channel_id: Uuid,
    pub agent_id: Option<Uuid>,
    pub status: TaskStatus,
    pub count: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TaskCountSnapshot {
    pub company: TaskCounts,
    pub channels: BTreeMap<Uuid, TaskCounts>,
    pub agents: BTreeMap<Uuid, TaskCounts>,
}

impl TaskCountSnapshot {
    pub fn from_rows(rows: impl IntoIterator<Item = TaskCountRow>) -> Self {
        let mut snapshot = Self::default();
        for row in rows {
            if !TaskStatus::OPEN.contains(&row.status) || row.count == 0 {
                continue;
            }
            snapshot.company.add(row.status, row.count);
            snapshot
                .channels
                .entry(row.channel_id)
                .or_default()
                .add(row.status, row.count);
            if let Some(agent_id) = row.agent_id {
                snapshot
                    .agents
                    .entry(agent_id)
                    .or_default()
                    .add(row.status, row.count);
            }
        }
        snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn folds_open_work_by_channel_and_current_agent_owner() {
        let channel = Uuid::new_v4();
        let other = Uuid::new_v4();
        let agent = Uuid::new_v4();
        let snapshot = TaskCountSnapshot::from_rows(
            TaskStatus::OPEN
                .into_iter()
                .map(|status| TaskCountRow {
                    channel_id: channel,
                    agent_id: Some(agent),
                    status,
                    count: 2,
                })
                .chain([
                    TaskCountRow {
                        channel_id: other,
                        agent_id: None,
                        status: TaskStatus::Pending,
                        count: 3,
                    },
                    TaskCountRow {
                        channel_id: other,
                        agent_id: Some(agent),
                        status: TaskStatus::Completed,
                        count: 99,
                    },
                ]),
        );
        assert_eq!(
            snapshot.company,
            TaskCounts {
                pending: 5,
                active: 2,
                waiting: 4
            }
        );
        assert_eq!(
            snapshot.agents[&agent],
            TaskCounts {
                pending: 2,
                active: 2,
                waiting: 4
            }
        );
        assert_eq!(
            snapshot
                .channels
                .values()
                .map(|c| c.pending + c.active + c.waiting)
                .sum::<u64>(),
            11
        );
        assert!(TaskCountSnapshot::from_rows([]).company.is_empty());
    }
}
