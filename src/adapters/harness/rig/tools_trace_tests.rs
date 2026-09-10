use super::*;
use crate::services::harness::{HarnessTrace, ToolTraceRecord};

#[derive(Default)]
struct Trace(std::sync::Mutex<Vec<Value>>);
#[async_trait]
impl HarnessTrace for Trace {
    async fn tool_started(&self, tool: &ToolId, argument_count: usize) {
        self.0
            .lock()
            .unwrap()
            .push(json!({"event":"start", "tool":tool, "argument_count":argument_count}));
    }
    async fn tool_finished(&self, record: ToolTraceRecord<'_>) {
        self.0
            .lock()
            .unwrap()
            .push(json!({"event":"finish", "tool":record.tool,
            "call_id":record.call_id,"outcome":record.outcome.label(),"executed":record.executed,
            "approval":record.approval}));
    }
    async fn approval_requested(&self, id: &str) {
        self.0
            .lock()
            .unwrap()
            .push(json!({"event":"approval", "id":id}));
    }
    async fn handoff(&self, _: &str, _: &str) {
        panic!("Rig has no delegation path")
    }
    async fn delegate_finished(&self, _: &str, _: &str, _: u64) {
        panic!("Rig has no delegation path")
    }
    async fn run_failed(&self) {
        self.0.lock().unwrap().push(json!({"event":"failed"}));
    }
}

#[tokio::test]
async fn trace_distinguishes_executed_denied_unknown_and_timed_out_tools_without_content() {
    let trace = Arc::new(Trace::default());
    let host = host(ToolInvocation::success(
        json!({"private-result":"private-credential"}),
    ));
    let mut bridge =
        ToolBridge::compile(&spec(&["list_company_agents"]), Some(host.clone())).unwrap();
    bridge.set_trace(Some(trace.clone()));
    bridge
        .invoke(
            &"private-tool-name".into(),
            &correlation(),
            json!({"private-args":true}),
        )
        .await
        .unwrap();
    bridge
        .invoke(&"list_company_agents".into(), &correlation(), json!({}))
        .await
        .unwrap();
    bridge.restrict_deadline(tokio::time::Instant::now());
    assert!(matches!(
        bridge
            .invoke(&"list_company_agents".into(), &correlation(), json!({}))
            .await,
        Err(AppError::Timeout(_))
    ));
    assert_eq!(host.calls.load(Ordering::SeqCst), 1);
    let events = trace.0.lock().unwrap();
    assert_eq!(events[0]["tool"], "unknown");
    assert_eq!(events[0]["executed"], false);
    assert_eq!(events[0]["outcome"], "not_executed");
    assert_eq!(events[2]["executed"], true);
    assert_eq!(events[2]["outcome"], "success");
    assert_eq!(events[3]["executed"], false);
    assert_eq!(events[3]["outcome"], "timed_out");
    assert!(
        !serde_json::to_string(&*events)
            .unwrap()
            .contains("private-")
    );
}

struct Decision(ApprovalVerdict);
#[async_trait]
impl HarnessApprovals for Decision {
    async fn decide(&self, _: ApprovalAsk<'_>) -> AppResult<ApprovalVerdict> {
        Ok(self.0.clone())
    }
}
#[tokio::test]
async fn waiting_for_approval_and_rejected_approval_do_not_report_execution_or_run_failure() {
    for decision in [
        ApprovalVerdict::pending("private reason"),
        ApprovalVerdict::rejected("private reason"),
    ] {
        let pending = matches!(decision, ApprovalVerdict::Pending { .. });
        let trace = Arc::new(Trace::default());
        let mut host = host(ToolInvocation::success(Value::Null));
        Arc::get_mut(&mut host).unwrap().declarations[0]
            .safety
            .requires_approval_by_default = true;
        let mut bridge = ToolBridge::compile(&spec(&["list_company_agents"]), Some(host.clone()))
            .unwrap()
            .with_approvals(Arc::new(Decision(decision)));
        bridge.set_trace(Some(trace.clone()));
        let result = bridge
            .invoke(&"list_company_agents".into(), &correlation(), json!({}))
            .await
            .unwrap();
        assert_eq!(result.suspends_run(), pending);
        assert_eq!(host.calls.load(Ordering::SeqCst), 0);
        let events = trace.0.lock().unwrap();
        assert_eq!(events.last().unwrap()["executed"], false);
        assert_eq!(
            events.last().unwrap()["outcome"],
            if pending { "suspended" } else { "not_executed" }
        );
        assert_eq!(
            events.last().unwrap()["approval"],
            if pending { "pending" } else { "rejected" }
        );
        assert_eq!(
            events.iter().filter(|e| e["event"] == "approval").count(),
            usize::from(pending)
        );
        assert!(!serde_json::to_string(&*events).unwrap().contains("private"));
    }
}

#[tokio::test(start_paused = true)]
async fn optional_telemetry_is_bounded_and_panics_cannot_change_a_committed_result() {
    super::super::super::trace::optional(std::future::pending()).await;
    super::super::super::trace::optional(async { panic!("optional telemetry failed") }).await;
}
