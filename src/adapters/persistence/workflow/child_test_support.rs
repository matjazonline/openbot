//! Child runs for persistence tests (07-D1 §D1.2a). Children exist only through the runtime
//! child writer, so tests reach them in one of two ways:
//! - category A, `chain`/`spawn`: the real path. Each ancestor's entry step is a `workflow.call`
//!   pinning the level below, and `park_child` parks it on its child.
//! - category B, `lineage_child`: a constructed state for SQL protocols the runtime never
//!   reaches (a live descendant beside an ancestor that is not parked on it, or several live
//!   descendants of one root).
use super::*;
use crate::application::workflow::activation::{ActivationRequest, WorkflowJobId};
use crate::application::workflow::children::WorkflowChildren;
use children::{ChildOrigin, ChildStart};

/// A workflow whose entry step calls the pinned child with the run input and then ends.
pub(super) fn call_source(child: WorkflowId, version: VersionId) -> Value {
    json!({
        "format_version": 1,
        "workflow_id": Uuid::nil().to_string(),
        "input_schema": true,
        "parameter_schema": true,
        "output_schema": true,
        "resources": [],
        "entry": "start",
        "steps": {"start": {
            "type": "workflow.call",
            "child_workflow_id": child.to_string(),
            "child_version_id": version.to_string(),
            "with": {"input": {"ref": "/input"}},
            "routes": {"success": "$end"}
        }},
        "limits": {"max_steps": 100, "max_context_bytes": 65536}
    })
}

impl AdmissionFixture {
    /// Category A: publish `leaf`, then `levels - 1` intermediate callers, then bind and admit a
    /// root that calls the top one, and park each level on its child. Returns the root first and
    /// the leaf child last; every ancestor is parked at its `workflow.call` step.
    pub(super) async fn chain(leaf: Value, levels: usize) -> (Self, Vec<ActivationRequest>) {
        Self::chain_in(Fixture::new().await, leaf, levels).await
    }

    /// `chain` in a prepared company, e.g. one whose members a root token policy names.
    pub(super) async fn chain_in(
        fixture: Fixture,
        leaf: Value,
        levels: usize,
    ) -> (Self, Vec<ActivationRequest>) {
        let (f, root) = Self::callers(fixture, leaf, levels).await;
        let mut scopes = vec![root];
        for _ in 0..levels {
            let parent = *scopes.last().unwrap();
            scopes.push(f.spawn(parent).await);
        }
        (f, scopes)
    }

    /// A root whose entry `workflow.call` pins `leaf`, admitted but not yet parked.
    pub(super) async fn caller(leaf: Value) -> (Self, ActivationRequest) {
        Self::callers(Fixture::new().await, leaf, 1).await
    }

    /// A bound root whose entry `workflow.call` pins `leaf`, not yet admitted, so the caller can
    /// admit it with any trigger and association.
    pub(super) async fn calling(leaf: Value) -> Self {
        Self::bound_callers(Fixture::new().await, leaf, 1).await
    }

    async fn callers(fixture: Fixture, leaf: Value, levels: usize) -> (Self, ActivationRequest) {
        let f = Self::bound_callers(fixture, leaf, levels).await;
        let command = f.prepare(f.request("chain-root", f.manual())).await;
        f.persistence().admit(&command).await.unwrap();
        let root = entry(&f, command.proposed_run_id()).await;
        (f, root)
    }

    async fn bound_callers(fixture: Fixture, leaf: Value, levels: usize) -> Self {
        assert!(levels >= 1);
        // Callers state the leaf's limits, so a root budget declared on it governs the tree.
        let limits = leaf["limits"].clone();
        let call = |workflow, version| {
            let mut source = call_source(workflow, version);
            source["limits"] = limits.clone();
            source
        };
        let mut workflow = WorkflowId::new(Uuid::from_u128(0x0700 + levels as u128));
        let mut version = fixture.publish_workflow(workflow, leaf).await;
        for level in (1..levels).rev() {
            let caller = WorkflowId::new(Uuid::from_u128(0x0700 + level as u128));
            version = fixture
                .publish_workflow(caller, call(workflow, version))
                .await;
            workflow = caller;
        }
        let mut root = call(workflow, version);
        root["workflow_id"] = json!(fixture.target.workflow.to_string());
        Self::with_binding(BindingFixture::from_fixture(fixture, root).await).await
    }

    /// Publish `child`, then a root built by `parent` from the child's pinned identity; bind and
    /// admit the root without parking it.
    pub(super) async fn with_child(
        child: Value,
        parent: impl FnOnce(WorkflowId, VersionId) -> Value,
    ) -> (Self, ActivationRequest) {
        Self::with_bound_child(child, parent, json!({}), &[]).await
    }

    /// `with_child`, binding the root with `params` and each of `slots` to its own new MCP
    /// connection.
    pub(super) async fn with_bound_child(
        child: Value,
        parent: impl FnOnce(WorkflowId, VersionId) -> Value,
        params: Value,
        slots: &[&str],
    ) -> (Self, ActivationRequest) {
        let fixture = Fixture::new().await;
        let workflow = WorkflowId::new(Uuid::from_u128(0x0701));
        let version = fixture.publish_workflow(workflow, child).await;
        let mut root = parent(workflow, version);
        root["workflow_id"] = json!(fixture.target.workflow.to_string());
        let mut binding = BindingFixture::from_fixture(fixture, root).await;
        binding.params = params;
        for slot in slots {
            binding.mcp_slot(slot).await;
        }
        let f = Self::with_binding(binding).await;
        let command = f.prepare(f.request("child-root", f.manual())).await;
        f.persistence().admit(&command).await.unwrap();
        let root = entry(&f, command.proposed_run_id()).await;
        (f, root)
    }

    /// A bound `data.map` workflow of another company in the same database.
    pub(super) async fn foreign(&self) -> AdmissionFixture {
        let fixture = Fixture::in_database(self.binding.fixture._db.clone(), "-foreign").await;
        let source = serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
        Self::with_binding(BindingFixture::from_fixture(fixture, source).await).await
    }

    /// Another bound `data.map` workflow, `workflow`, of the same company and actor.
    pub(super) async fn sibling(&self, workflow: WorkflowId) -> AdmissionFixture {
        let fixture = Fixture {
            _db: self.binding.fixture._db.clone(),
            persistence: self.persistence().clone(),
            target: self.binding.fixture.target,
        };
        let source = serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
        let version = fixture.publish_workflow(workflow, source).await;
        let fixture = Fixture {
            target: DefinitionTarget {
                workflow,
                ..fixture.target
            },
            ..fixture
        };
        Self::with_binding(BindingFixture::from_version(fixture, version)).await
    }

    /// The child named by `parent`'s waiting `child_run` wait (the current round).
    pub(super) async fn current_child(
        &self,
        parent: ActivationRequest,
    ) -> Option<ActivationRequest> {
        let child: Option<Uuid> = sqlx::query_scalar("SELECT child_run_id FROM workflow_waits WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND reason='child_run' AND state='waiting'")
            .bind(parent.company.as_uuid()).bind(parent.run.as_uuid()).bind(parent.execution.as_uuid())
            .fetch_optional(self.persistence().pool()).await.unwrap();
        match child {
            Some(child) => Some(entry(self, RunId::new(child)).await),
            None => None,
        }
    }

    /// Park `parent` on its child step and return the child's entry scope.
    pub(super) async fn spawn(&self, parent: ActivationRequest) -> ActivationRequest {
        let parked = self
            .persistence()
            .park_child(parent)
            .await
            .unwrap()
            .expect("the parent parks on its child");
        let child: Uuid = sqlx::query_scalar(
            "SELECT child_run_id FROM workflow_waits WHERE company_id=$1 AND id=$2",
        )
        .bind(parent.company.as_uuid())
        .bind(parked.wait.as_uuid())
        .fetch_one(self.persistence().pool())
        .await
        .unwrap();
        entry(self, RunId::new(child)).await
    }

    /// Category B: a lineage-only child of `parent`, which keeps running as if it had none.
    /// It is written by the production `children::create_on` with the parent run's own bundle,
    /// configuration and input, and its parent wait is inserted already settled (`cancelled`)
    /// in the same transaction, satisfying `workflow_child_parent_wait`. Contract (WF-P07-F012):
    /// - the parent step must not be a wait, decision or child step, whose park would replay the
    ///   settled wait; only those steps read it, and a resume replaying it answers `Refused`;
    /// - each lineage child of one execution takes the next free ordinal;
    /// - inserting the wait advances the parent run's `revision`;
    /// - the child is outside the cancel cascade and its wakeup fact is never consumed, so a
    ///   test must not rely on either.
    pub(super) async fn lineage_child(&self, parent: ActivationRequest) -> ActivationRequest {
        let p = self.persistence();
        let mut tx = p.pool().begin().await.unwrap();
        let (binding_id, input, deadline, step): (Uuid, Value, chrono::DateTime<chrono::Utc>, String) =
            sqlx::query_as("SELECT run.binding_id, run.input, run.deadline, execution.step_id FROM workflow_runs AS run JOIN workflow_executions AS execution ON execution.company_id=run.company_id AND execution.run_id=run.id WHERE run.company_id=$1 AND run.id=$2 AND execution.id=$3")
                .bind(parent.company.as_uuid()).bind(parent.run.as_uuid()).bind(parent.execution.as_uuid())
                .fetch_one(&mut *tx).await.unwrap();
        let binding = admission_binding::read_saved_binding(
            &mut tx,
            parent.company,
            WorkflowBindingId::new(binding_id),
            parent.run.as_uuid(),
        )
        .await
        .unwrap();
        let step = StepId::parse(step).unwrap();
        let kind = binding.bundle().compiled().graph().definition().steps[&step]
            .step_type
            .as_str()
            .to_owned();
        assert!(
            !matches!(
                kind.as_str(),
                "wait.event" | "wait.timer" | "decision.human" | "workflow.call" | "flow.repeat"
            ),
            "a lineage child's parent step must not read its own waits"
        );
        let next: i32 = sqlx::query_scalar("SELECT COALESCE(MAX(child_ordinal),0)+1 FROM workflow_waits WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND reason='child_run'")
            .bind(parent.company.as_uuid()).bind(parent.run.as_uuid()).bind(parent.execution.as_uuid())
            .fetch_one(&mut *tx).await.unwrap();
        let parked = children::create_on(
            &mut tx,
            parent,
            &ChildStart {
                step: &step,
                origin: ChildOrigin::Round(ChildOrdinal::new(next as u32).unwrap()),
                binding: &binding,
                input: &input,
                deadline,
            },
        )
        .await
        .unwrap();
        sqlx::query("UPDATE workflow_waits SET state='cancelled',settled_at=clock_timestamp() WHERE company_id=$1 AND id=$2")
            .bind(parent.company.as_uuid()).bind(parked.wait.as_uuid())
            .execute(&mut *tx).await.unwrap();
        let child: Uuid = sqlx::query_scalar(
            "SELECT child_run_id FROM workflow_waits WHERE company_id=$1 AND id=$2",
        )
        .bind(parent.company.as_uuid())
        .bind(parked.wait.as_uuid())
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        entry(self, RunId::new(child)).await
    }
}

/// The entry execution and job of `run`.
pub(super) async fn entry(f: &AdmissionFixture, run: RunId) -> ActivationRequest {
    let (company, execution, job): (Uuid, Uuid, Uuid) = sqlx::query_as(
        "SELECT execution.company_id, execution.id, job.id FROM workflow_executions AS execution \
         JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id \
         WHERE execution.run_id=$1 AND execution.activation=1",
    )
    .bind(run.as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    ActivationRequest {
        company: CompanyId::new(company),
        run,
        execution: ExecutionId::new(execution),
        job: WorkflowJobId(job),
    }
}
