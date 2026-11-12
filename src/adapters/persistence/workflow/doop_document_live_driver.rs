//! One coordinator owns two competing worker groups and the shared delivery queue.
use super::*;
use futures::FutureExt;
use std::panic::AssertUnwindSafe;

#[derive(Clone, Copy)]
pub(super) enum Checkpoint {
    CreateApproval,
    EditorApproval,
    Terminal,
    Reconciliation,
}

pub(super) struct Driver<'a> {
    pub(super) environment: &'a setup::Environment,
    pub(super) documents: &'a [setup::CompanyDocument],
    pub(super) delivery: &'a delivery_support::DeliveryFixture,
}

impl Driver<'_> {
    pub(super) async fn drive(
        &self,
        engine: &WorkflowEngine,
        scopes: &[RunReadScope],
        checkpoint: Checkpoint,
        models: Option<(&mut [Option<ScriptedLlm>], &Input)>,
    ) -> Vec<RunView> {
        assert_eq!(engine.slots(), 1);
        let shutdown = CancellationToken::new();
        let _guard = shutdown.clone().drop_guard();
        let mut workers = engine.spawn(&shutdown);
        workers.extend(engine.spawn(&shutdown));
        let steps = document_source(&self.documents[0].fixture)["steps"]
            .as_object()
            .unwrap()
            .len() as u64;
        // Preserve the existing per-graph fixture bound; the outer native helper is still80s.
        let result = tokio::time::timeout(
            Duration::from_secs(5 + 3 * steps),
            AssertUnwindSafe(self.poll(engine, scopes, checkpoint, models)).catch_unwind(),
        )
        .await;
        shutdown.cancel();
        for (_, worker) in workers {
            tokio::time::timeout(Duration::from_secs(10), worker)
                .await
                .unwrap()
                .unwrap();
        }
        match result {
            Ok(Ok(views)) => views,
            Ok(Err(panic)) => std::panic::resume_unwind(panic),
            Err(_) => panic!("live document checkpoint exceeded its existing graph bound"),
        }
    }

    async fn poll(
        &self,
        engine: &WorkflowEngine,
        scopes: &[RunReadScope],
        checkpoint: Checkpoint,
        mut models: Option<(&mut [Option<ScriptedLlm>], &Input)>,
    ) -> Vec<RunView> {
        let (_delivery_shutdown, mut receiver) = tokio::sync::broadcast::channel(1);
        let mut previous = vec![None; scopes.len()];
        loop {
            self.delivery
                .worker
                .drain_once(&mut receiver)
                .await
                .unwrap();
            let mut ready = Vec::new();
            for (scope_index, scope) in scopes.iter().enumerate() {
                let (index, document) = self
                    .documents
                    .iter()
                    .enumerate()
                    .find(|(_, document)| document.target.company == scope.company)
                    .unwrap();
                let view = self
                    .environment
                    .persistence
                    .run_view(*scope)
                    .await
                    .unwrap()
                    .unwrap();
                if let Some((models, input)) = models.as_mut() {
                    assert_eq!(models.len(), scopes.len());
                    // The persisted editor completion proves its final response was consumed.
                    // Close that script now; downstream reads/notices need no model server.
                    if view.detail.executions.iter().any(|execution| {
                        execution.step == "edit" && execution.completed_at.is_some()
                    }) && let Some(model) = models[scope_index].take()
                    {
                        Box::pin(finish_model(model, input)).await;
                    }
                }
                // Only committed progress hints the existing worker wake API. Claims,
                // fairness, the positive scan delay and all deadlines stay authoritative.
                if previous[scope_index].as_ref() != Some(&view.detail) {
                    engine.services().wake_workers();
                    previous[scope_index] = Some(view.detail.clone());
                }
                let runtime = document.runtime(engine);
                let decisions = runtime.approvals(*scope).await;
                if self.reached(index, &view, &decisions, checkpoint) {
                    ready.push(view);
                } else if view.detail.summary.state == "waiting" {
                    for decision in decisions {
                        runtime.approve(*scope, decision).await;
                    }
                }
            }
            // A persisted approval can precede the active handler returning. Let it
            // finish parking before cancellation, rather than abandoning its lease.
            if ready.len() == scopes.len() && engine.active_jobs().current() == 0 {
                eprintln!("DOOP_E2E_CHECKPOINT workers_idle=true");
                return ready;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    fn reached(
        &self,
        index: usize,
        view: &RunView,
        decisions: &[DecisionId],
        checkpoint: Checkpoint,
    ) -> bool {
        match checkpoint {
            Checkpoint::CreateApproval => {
                assert!(
                    !terminal(view),
                    "the initial create must require its real approval"
                );
                let effects = self.environment.client.company(index);
                view.detail.summary.state == "waiting"
                    && !decisions.is_empty()
                    && effects.canvases.is_empty()
                    && effects.count("get_profile") == 1
            }
            Checkpoint::Terminal => terminal(view),
            Checkpoint::Reconciliation => {
                assert!(
                    !terminal(view),
                    "lost receipt must not claim success or terminal failure"
                );
                view.detail.summary.waiting_reason.as_deref() == Some("reconciliation")
            }
            Checkpoint::EditorApproval => {
                assert!(
                    !terminal(view),
                    "the editor must reach a real pending write approval"
                );
                let effects = self.environment.client.company(index);
                view.detail.summary.state == "waiting"
                    && !decisions.is_empty()
                    && effects.canvases.len() == 1
                    && effects.count("get_canvas") >= 2
                    && effects.frame.is_none()
            }
        }
    }
}
