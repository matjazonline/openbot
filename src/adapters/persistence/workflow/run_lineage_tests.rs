//! Plan `fix-02-run-child-duplicates` at the read port: `RunLineage::TopLevel` leaves child runs
//! out in SQL, so pages stay full, and `child_rollup` counts each parent's direct children by
//! state in one read. Children come from the production child writer (`child_support`).
use super::child_support::entry;
use super::*;
use crate::application::workflow::activation::ActivationRequest;
use crate::application::workflow::reads::{
    ChildCounts, MAX_ROLLUP_PARENTS, PageRequest, RunFilter, RunLineage, WorkflowReads,
};

fn data_map() -> Value {
    serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap()
}

/// A root admitted on `f`'s binding under `key`: its entry scope.
async fn admitted(f: &AdmissionFixture, key: &str) -> ActivationRequest {
    let command = f.prepare(f.request(key, f.manual())).await;
    f.persistence().admit(&command).await.unwrap();
    entry(f, command.proposed_run_id()).await
}

/// One page of `f`'s company's runs: the ids, and whether more follow.
async fn listed(
    f: &AdmissionFixture,
    lineage: RunLineage,
    after: Option<Uuid>,
    size: i64,
) -> (Vec<Uuid>, bool) {
    let page = f
        .persistence()
        .list_runs(
            f.binding.target.company,
            &RunFilter {
                lineage,
                ..Default::default()
            },
            PageRequest::new(after, Some(size)).unwrap(),
        )
        .await
        .unwrap();
    (
        page.items.iter().map(|run| run.run.as_uuid()).collect(),
        page.more,
    )
}

async fn set_state(f: &AdmissionFixture, run: RunId, state: &str) {
    sqlx::query("UPDATE workflow_runs SET state = $2 WHERE company_id = $1 AND id = $3")
        .bind(f.binding.target.company.as_uuid())
        .bind(state)
        .bind(run.as_uuid())
        .execute(f.persistence().pool())
        .await
        .unwrap();
}

/// A child is left out; a rerun, which is always top-level, stays. Every run is the default, as the
/// list has always read.
#[tokio::test]
async fn run_top_level_leaves_out_children_and_keeps_reruns() {
    let (f, scopes) = AdmissionFixture::chain(data_map(), 1).await;
    let (root, child) = (scopes[0].run.as_uuid(), scopes[1].run.as_uuid());
    let mut request = f.request("rerun", f.manual());
    request.rerun_of = Some(scopes[0].run);
    let command = f.prepare(request).await;
    f.persistence().admit(&command).await.unwrap();
    let rerun = command.proposed_run_id().as_uuid();

    assert_eq!(RunFilter::default().lineage, RunLineage::Any);
    assert_eq!(
        listed(&f, RunLineage::Any, None, 50).await.0,
        [rerun, child, root]
    );
    assert_eq!(
        listed(&f, RunLineage::TopLevel, None, 50).await.0,
        [rerun, root]
    );
}

/// Children interleaved between roots never short a top-level page or let the cursor skip a root.
#[tokio::test]
async fn run_top_level_pages_stay_full_with_children_interleaved() {
    let f = AdmissionFixture::new().await;
    let first = admitted(&f, "first").await;
    f.lineage_child(first).await;
    f.lineage_child(first).await;
    let second = admitted(&f, "second").await;
    f.lineage_child(second).await;
    let third = admitted(&f, "third").await;
    assert_eq!(listed(&f, RunLineage::Any, None, 50).await.0.len(), 6);

    let mut paged = Vec::new();
    let mut after = None;
    loop {
        let (items, more) = listed(&f, RunLineage::TopLevel, after, 1).await;
        assert_eq!(items.len(), 1, "every page is full");
        paged.push(items[0]);
        if !more {
            break;
        }
        after = Some(items[0]);
    }
    assert_eq!(
        paged,
        [third.run, second.run, first.run].map(|run| run.as_uuid())
    );
}

/// Direct children only, by state: a parked chain counts one child at each level and none on the
/// leaf; children in three states count as open or failed; a parent without children is absent.
#[tokio::test]
async fn run_top_level_rollup_counts_direct_children_by_state() {
    let (f, scopes) = AdmissionFixture::chain(data_map(), 2).await;
    let (root, middle, leaf) = (scopes[0].run, scopes[1].run, scopes[2].run);
    let siblings = f.sibling(WorkflowId::new(Uuid::new_v4())).await;
    let busy = admitted(&siblings, "busy").await;
    let queued = siblings.lineage_child(busy).await.run;
    let running = siblings.lineage_child(busy).await.run;
    let failed = siblings.lineage_child(busy).await.run;
    set_state(&siblings, running, "running").await;
    set_state(&siblings, failed, "failed").await;
    let idle = admitted(&siblings, "idle").await.run;

    let rollup = f
        .persistence()
        .child_rollup(
            f.binding.target.company,
            &[root, middle, leaf, busy.run, idle, queued],
        )
        .await
        .unwrap();
    let one_open = ChildCounts {
        total: 1,
        open: 1,
        failed: 0,
    };
    assert_eq!(rollup.get(&root), Some(&one_open), "the middle run only");
    assert_eq!(rollup.get(&middle), Some(&one_open));
    assert_eq!(
        rollup.get(&busy.run),
        Some(&ChildCounts {
            total: 3,
            open: 2,
            failed: 1
        })
    );
    assert_eq!(rollup.len(), 3, "{rollup:?}");
}

/// The rollup reads the company it is given: another company's parents count nothing through it,
/// and this company's parents count nothing through another.
#[tokio::test]
async fn run_top_level_rollup_reads_only_the_companys_runs() {
    let (f, scopes) = AdmissionFixture::chain(data_map(), 1).await;
    let ours = f.binding.target.company;
    let foreign = f.foreign().await;
    let theirs = foreign.binding.target.company;
    let their_root = admitted(&foreign, "theirs").await;
    foreign.lineage_child(their_root).await;

    let p = f.persistence();
    assert_eq!(
        p.child_rollup(ours, &[scopes[0].run])
            .await
            .unwrap()
            .get(&scopes[0].run)
            .map(|counts| counts.total),
        Some(1)
    );
    assert!(
        p.child_rollup(ours, &[their_root.run])
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        p.child_rollup(theirs, &[scopes[0].run])
            .await
            .unwrap()
            .is_empty()
    );
}

/// More parents than one board read shows is refused, not cut.
#[tokio::test]
async fn run_top_level_rollup_refuses_more_parents_than_its_bound() {
    let f = AdmissionFixture::new().await;
    let company = f.binding.target.company;
    let parents: Vec<RunId> = (0..=MAX_ROLLUP_PARENTS)
        .map(|_| RunId::new(Uuid::new_v4()))
        .collect();
    assert!(matches!(
        f.persistence().child_rollup(company, &parents).await,
        Err(AppError::BadRequest(_))
    ));
    assert!(
        f.persistence()
            .child_rollup(company, &parents[..MAX_ROLLUP_PARENTS])
            .await
            .unwrap()
            .is_empty()
    );
}
