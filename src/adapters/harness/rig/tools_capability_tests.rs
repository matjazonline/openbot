use super::*;
use crate::entities::{
    creation::CreationProvenance,
    mcp::{McpToolName, McpToolRef},
    skill::{Skill, SkillInstruction},
    value_objects::SkillSlug,
};
use crate::services::approval_tool::RequestApprovalTool;
use uuid::Uuid;

fn skill(id: &str) -> Skill {
    Skill {
        id: Uuid::new_v4(),
        company_id: None,
        slug: SkillSlug::parse("read").unwrap(),
        name: "Read".into(),
        description: "Read".into(),
        trigger: "Read".into(),
        instructions: vec![SkillInstruction::Tool {
            tool: id.into(),
            args: None,
            output_as: None,
        }],
        created_by: CreationProvenance::system(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

#[test]
fn every_real_native_declaration_compiles_without_schema_or_name_translation() {
    use crate::services::{
        agent_channel_tool::CreateAgentChannelTool, agent_directory_tool::ListCompanyAgentsTool,
        outreach_tool::OutreachAndAwaitQuorumTool, task_ownership_tool::TaskOwnershipTool,
    };
    for declaration in [
        CreateAgentChannelTool::declaration(),
        ListCompanyAgentsTool::declaration(),
        OutreachAndAwaitQuorumTool::declaration(),
        TaskOwnershipTool::declaration(),
        RequestApprovalTool::declaration(),
    ] {
        assert!(
            super::super::super::schema::compile(&declaration.input_schema).is_ok(),
            "{}",
            declaration.id
        );
    }
}

#[tokio::test]
async fn skill_implied_grants_share_dispatch_and_missing_context_fails_preflight() {
    let mut spec = spec(&[]);
    spec.skills = vec![skill("echo")];
    let bridge = ToolBridge::compile(&spec, None).unwrap();
    assert!(
        bridge
            .invoke(
                &"echo".into(),
                &correlation(),
                json!({"message":"skill result"})
            )
            .await
            .unwrap()
            .success
    );
    spec.skills = vec![skill("list_company_agents")];
    assert!(ToolBridge::compile(&spec, None).is_err());
    spec.skills = vec![skill("request_approval")];
    assert!(ToolBridge::compile(&spec, None).is_err());
}

#[test]
fn checkpoint_grants_are_rejected_on_incompatible_direct_and_skill_writes() {
    use crate::use_cases::agent::{AgentWrite, validate_effective_capabilities};
    let mut write = AgentWrite {
        name: "Agent".into(),
        slug: "agent".into(),
        harness_kind: Some(HarnessKind::AiAgents),
        granted_tool_ids: vec!["request_approval".into()],
        ..Default::default()
    };
    assert!(write.normalize().is_err());
    write.granted_tool_ids.clear();
    assert!(validate_effective_capabilities(&write, &[skill("request_approval")]).is_err());
    write.harness_kind = Some(HarnessKind::Rig);
    write.granted_tool_ids.push("request_approval".into());
    assert!(write.normalize().is_ok());
    assert!(validate_effective_capabilities(&write, &[skill("request_approval")]).is_ok());
}

struct Checkpoints(AtomicUsize);
#[async_trait]
impl HarnessApprovals for Checkpoints {
    async fn decide(&self, ask: ApprovalAsk<'_>) -> AppResult<ApprovalVerdict> {
        assert!(ask.invocation.is_some());
        let ApprovalTrigger::Checkpoint { proposal, .. } = ask.trigger else {
            panic!("Unexpected generic approval");
        };
        assert_eq!(proposal, "Concrete proposal");
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(ApprovalVerdict::Pending {
            reason: "Fixture pending".into(),
        })
    }
}

struct NoAutomaticApprovals;
#[async_trait]
impl HarnessApprovals for NoAutomaticApprovals {
    async fn decide(&self, _: ApprovalAsk<'_>) -> AppResult<ApprovalVerdict> {
        panic!("MCP and explicit checkpoints must bypass the generic approval gate");
    }
}

#[tokio::test]
async fn explicit_checkpoint_uses_native_dispatch_and_owns_its_single_approval() {
    let checkpoints = Arc::new(Checkpoints(AtomicUsize::new(0)));
    let bridge = ToolBridge::compile_with_approvals(
        &spec(&["request_approval"]),
        None,
        Some(checkpoints.clone()),
    )
    .unwrap();
    let reference = crate::entities::harness_run::InvocationRef {
        run_id: crate::entities::harness_run::RunId(Uuid::new_v4()),
        invocation_id: crate::entities::harness_run::InvocationId(Uuid::new_v4()),
        expected_revision: crate::entities::harness_run::CheckpointRevision(1),
    };
    let result = bridge
        .invoke_saved(
            &"request_approval".into(),
            &ToolCorrelationId::parse(&reference.invocation_id.0.to_string()).unwrap(),
            json!({"title":"Review","proposal":"Concrete proposal"}),
            Some(reference),
        )
        .await
        .unwrap();
    assert!(result.suspends_run());
    assert_eq!(checkpoints.0.load(Ordering::SeqCst), 1);
    assert!(
        bridge
            .invoke(&"request_approval".into(), &correlation(), json!({}))
            .await
            .is_err()
    );
    assert!(
        !CatalogueTool::get(&"request_approval".into())
            .unwrap()
            .supports_harness(HarnessKind::AiAgents)
    );
}

struct McpHost {
    declarations: Vec<McpToolDeclaration>,
    calls: AtomicUsize,
}
#[async_trait]
impl HarnessMcpToolHost for McpHost {
    fn available(&self) -> &[McpToolDeclaration] {
        &self.declarations
    }
    async fn invoke(
        &self,
        declaration: &McpToolDeclaration,
        _: &str,
        args: Value,
    ) -> AppResult<ToolInvocation> {
        assert!(
            self.declarations
                .iter()
                .any(|granted| granted.identity == declaration.identity)
        );
        assert_eq!(declaration.selection_revision, 7);
        assert_eq!(declaration.definition_revision, 3);
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ToolInvocation::success(args))
    }
}

fn mcp() -> Arc<McpHost> {
    Arc::new(McpHost { calls: AtomicUsize::new(0), declarations: (0..2).map(|_| McpToolDeclaration {
        identity: McpToolRef { connection_id: Uuid::new_v4(), name: McpToolName::try_from("search".to_string()).unwrap() },
        description: "Remote search".into(), definition_revision: 3, credential_revision: 1, selection_revision: 7,
        input_schema: json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"],"additionalProperties":false}),
    }).collect() })
}

#[tokio::test]
async fn mcp_connection_identity_grants_schemas_and_no_approval_are_enforced() {
    let host = mcp();
    let bridge = ToolBridge::compile(&spec(&[]), None)
        .unwrap()
        .with_mcp(host.clone())
        .unwrap()
        .with_approvals(Arc::new(NoAutomaticApprovals));
    let ids: Vec<_> = host
        .declarations
        .iter()
        .map(|d| mcp_model_id(&d.identity))
        .collect();
    assert_ne!(ids[0], ids[1]);
    for id in &ids {
        assert_eq!(id.len(), 52);
        assert!(
            bridge
                .invoke(id, &correlation(), json!({"query":"record"}))
                .await
                .unwrap()
                .success
        );
        assert!(
            !bridge
                .invoke(id, &correlation(), json!({"query":42}))
                .await
                .unwrap()
                .success
        );
        assert!(
            !bridge
                .invoke(
                    id,
                    &correlation(),
                    json!({"query":"record","endpoint":"https://evil.example"})
                )
                .await
                .unwrap()
                .success
        );
    }
    assert!(
        !bridge
            .invoke(&"search".into(), &correlation(), json!({"query":"record"}))
            .await
            .unwrap()
            .success
    );
    assert_eq!(host.calls.load(Ordering::SeqCst), 2);
    assert!(CatalogueTool::get(&ids[0]).is_none());
    let mut host = mcp();
    Arc::get_mut(&mut host).unwrap().declarations[0].input_schema =
        json!({"$ref":"file:///etc/passwd"});
    assert!(
        ToolBridge::compile(&spec(&[]), None)
            .unwrap()
            .with_mcp(host)
            .is_err()
    );
}

struct Decisions(ApprovalVerdict);
#[async_trait]
impl HarnessApprovals for Decisions {
    async fn decide(&self, _: ApprovalAsk<'_>) -> AppResult<ApprovalVerdict> {
        Ok(self.0.clone())
    }
}

#[tokio::test]
async fn protected_native_tools_require_an_explicit_decision() {
    for verdict in [
        ApprovalVerdict::Approved,
        ApprovalVerdict::pending("wait"),
        ApprovalVerdict::rejected("no"),
    ] {
        let mut host = host(ToolInvocation::success(json!("ok")));
        Arc::get_mut(&mut host).unwrap().declarations[0]
            .safety
            .requires_approval_by_default = true;
        let bridge = ToolBridge::compile(&spec(&["list_company_agents"]), Some(host.clone()))
            .unwrap()
            .with_approvals(Arc::new(Decisions(verdict.clone())));
        let result = bridge
            .invoke(&"list_company_agents".into(), &correlation(), json!({}))
            .await
            .unwrap();
        assert_eq!(
            host.calls.load(Ordering::SeqCst),
            usize::from(verdict == ApprovalVerdict::Approved)
        );
        assert_eq!(
            result.suspends_run(),
            matches!(verdict, ApprovalVerdict::Pending { .. })
        );
    }
}
