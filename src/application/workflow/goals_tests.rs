use super::*;
use uuid::Uuid;

fn unit(kind: DepartmentKind) -> GoalCandidate {
    GoalCandidate {
        unit: DepartmentId::new(Uuid::new_v4()),
        kind,
        archived: false,
    }
}

fn goal() -> GoalCandidate {
    unit(DepartmentKind::Goal)
}

fn standing() -> GoalCandidate {
    unit(DepartmentKind::Standing)
}

fn archived(mut candidate: GoalCandidate) -> GoalCandidate {
    candidate.archived = true;
    candidate
}

#[test]
fn goal_of_none_without_sources() {
    assert_eq!(goal_of(&GoalSources::default()), None);
    let sources = GoalSources {
        chain: vec![standing(), standing()],
        ..GoalSources::default()
    };
    assert_eq!(goal_of(&sources), None);
}

#[test]
fn goal_of_parent_wins_over_thread_and_desk() {
    let (parent, thread, desk) = (goal(), goal(), goal());
    let sources = GoalSources {
        parent: Some(parent),
        thread: Some(thread),
        chain: vec![desk],
        ..GoalSources::default()
    };
    assert_eq!(goal_of(&sources), Some(parent.unit));
}

#[test]
fn goal_of_thread_wins_over_desk() {
    let (thread, desk) = (goal(), goal());
    let sources = GoalSources {
        thread: Some(thread),
        chain: vec![desk],
        ..GoalSources::default()
    };
    assert_eq!(goal_of(&sources), Some(thread.unit));
}

#[test]
fn goal_of_nested_goals_pick_the_nearest() {
    let (inner, outer) = (goal(), goal());
    let sources = GoalSources {
        chain: vec![standing(), inner, standing(), outer],
        ..GoalSources::default()
    };
    assert_eq!(goal_of(&sources), Some(inner.unit));
}

#[test]
fn goal_of_archived_candidate_falls_through() {
    let desk = goal();
    let sources = GoalSources {
        parent: Some(archived(goal())),
        thread: Some(archived(goal())),
        chain: vec![archived(goal()), desk],
        ..GoalSources::default()
    };
    assert_eq!(goal_of(&sources), Some(desk.unit));
}

#[test]
fn goal_of_non_goal_candidate_falls_through() {
    // A goal that became standing after it was recorded no longer counts (D1 r2, F1).
    let thread = goal();
    let sources = GoalSources {
        parent: Some(standing()),
        thread: Some(thread),
        ..GoalSources::default()
    };
    assert_eq!(goal_of(&sources), Some(thread.unit));
}

#[test]
fn goal_of_rerun_comes_first() {
    let original = goal();
    let sources = GoalSources {
        rerun: Some(original),
        parent: Some(goal()),
        thread: Some(goal()),
        chain: vec![goal()],
    };
    assert_eq!(goal_of(&sources), Some(original.unit));
}

#[test]
fn goal_of_rerun_falls_through_when_original_goal_closed() {
    let desk = goal();
    for original in [archived(goal()), standing()] {
        let sources = GoalSources {
            rerun: Some(original),
            chain: vec![desk],
            ..GoalSources::default()
        };
        assert_eq!(goal_of(&sources), Some(desk.unit));
    }
}

#[test]
fn goal_of_depth_bound_ignores_units_past_the_tree_limit() {
    let mut chain = vec![standing(); MAX_DEPARTMENT_DEPTH];
    chain.push(goal());
    let sources = GoalSources {
        chain,
        ..GoalSources::default()
    };
    assert_eq!(goal_of(&sources), None);
    let mut chain = vec![standing(); MAX_DEPARTMENT_DEPTH - 1];
    let last = goal();
    chain.push(last);
    let sources = GoalSources {
        chain,
        ..GoalSources::default()
    };
    assert_eq!(goal_of(&sources), Some(last.unit));
}
