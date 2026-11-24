//! Which goal a run serves (Plan08 03B.2, gate 08G-P03-D1 r2 D1.4).
//!
//! The trail answers "on whose behalf", so a run inherits before it looks at where it runs: the
//! goal of the run it reruns, then its parent's (or a native call's source run), then its
//! thread's, then the nearest goal unit above its desk. A candidate that is archived or no longer
//! a goal counts as none and resolution moves on; persistence reads every candidate under a share
//! lock, so the answer and the insert guard agree.
use crate::entities::department::{DepartmentId, DepartmentKind, MAX_DEPARTMENT_DEPTH};

/// A unit named by an inherited source: whether it is still a live goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoalCandidate {
    pub unit: DepartmentId,
    pub kind: DepartmentKind,
    pub archived: bool,
}

impl GoalCandidate {
    fn live(self) -> Option<DepartmentId> {
        (self.kind == DepartmentKind::Goal && !self.archived).then_some(self.unit)
    }
}

/// The sources a run's goal comes from, each `None` when it does not apply.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GoalSources {
    /// The original run's goal, for a rerun.
    pub rerun: Option<GoalCandidate>,
    /// The parent run's goal for a child; a native call's source run's goal.
    pub parent: Option<GoalCandidate>,
    /// The goal recorded on the run's existing thread.
    pub thread: Option<GoalCandidate>,
    /// The desk's unit, then its ancestors, nearest first.
    pub chain: Vec<GoalCandidate>,
}

/// The first live goal among the sources, in order; the chain is read to the tree's depth bound.
pub fn goal_of(sources: &GoalSources) -> Option<DepartmentId> {
    [sources.rerun, sources.parent, sources.thread]
        .into_iter()
        .flatten()
        .chain(sources.chain.iter().copied().take(MAX_DEPARTMENT_DEPTH))
        .find_map(GoalCandidate::live)
}

#[cfg(test)]
#[path = "goals_tests.rs"]
mod tests;
