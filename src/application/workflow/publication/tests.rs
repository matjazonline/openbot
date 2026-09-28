use super::*;
use crate::adapters::workflow_source::decode;
use crate::domain::workflow::{ResourceName, RunId, RunMetadata, TypeName};
use uuid::Uuid;

#[path = "diagnostic_tests.rs"]
mod diagnostic_tests;

fn company() -> CompanyId {
    CompanyId::new(Uuid::from_u128(100))
}
fn name(value: &str) -> TypeName {
    TypeName::parse(value).unwrap()
}
fn source(kind: &str) -> Value {
    serde_json::from_str(&registry::example(kind).unwrap().source).unwrap()
}
fn agent() -> AgentSnapshot {
    AgentSnapshot {
        company_id: company(),
        key: AgentKey::parse("assistant").unwrap(),
        instructions: "Answer using the supplied context".into(),
        model: ModelSettings {
            provider: name("test"),
            model: "scripted".into(),
            max_output_tokens: 1000,
        },
        tools: vec![],
        skills: vec![],
    }
}
fn snapshots() -> DependencySnapshots {
    DependencySnapshots {
        agents: vec![agent()],
        ..Default::default()
    }
}
fn build(source: &Value, snapshots: DependencySnapshots) -> Result<PublishedBundle, Diagnostic> {
    freeze(
        decode(&source.to_string())?,
        company(),
        VersionId::new(Uuid::from_u128(10)),
        snapshots,
        vec![],
    )
}
fn tool_snapshot(kind: &str) -> ToolSnapshot {
    ToolSnapshot {
        company_id: company(),
        contract: registry::example(kind).unwrap().facts.tools.remove(0),
        policy: ApprovedActionPolicy {
            capability: name("customer.read"),
            policy_revision: 1,
            effect: ActionEffect::Read,
            recovery: ActionRecovery::SafeRepeat,
        },
    }
}

#[test]
fn workflow_publication_freezes_agents_skills_profiles_and_hashes() {
    let source = source("agent.run");
    let mut facts = snapshots();
    facts.tools.push(tool_snapshot("tool.call"));
    facts.skills.push(SkillSnapshot {
        company_id: company(),
        name: name("support"),
        instructions: "Follow the support policy".into(),
        required_tools: vec![name("lookup")],
    });
    facts.agents[0].tools.push(name("lookup"));
    facts.agents[0].skills.push(name("support"));
    facts.profiles.push(registry::CapabilityProfile {
        name: name("none"),
        tools: vec![],
        skills: vec![],
    });
    let original = build(&source, facts.clone()).unwrap();
    let original_manifest = original.manifest().clone();
    let identical = build(&source, facts.clone()).unwrap();
    assert_eq!(original.content_hash(), identical.content_hash());
    for edit in 0..5 {
        let mut changed = facts.clone();
        match edit {
            0 => changed.agents[0].instructions.push_str(" updated"),
            1 => changed.skills[0].instructions.push_str(" updated"),
            2 => changed.agents[0].model.max_output_tokens += 1,
            3 => changed.profiles[0].tools.push(name("lookup")),
            _ => changed.tools[0].policy.policy_revision += 1,
        }
        let later = build(&source, changed).unwrap();
        assert_ne!(original.content_hash(), later.content_hash());
        assert_eq!(original.manifest(), &original_manifest);
    }
    assert_eq!(original.snapshots().agents[0].skills, vec![name("support")]);
    assert!(original.manifest()["dependency_hashes"]["agents"][0].is_string());
}

#[test]
fn workflow_publication_rejects_incomplete_foreign_duplicate_and_oversized_dependencies() {
    let source = source("agent.run");
    for variant in 0..8 {
        let mut facts = snapshots();
        match variant {
            0 => facts.agents.clear(),
            1 => facts.agents[0].company_id = CompanyId::new(Uuid::nil()),
            2 => facts.agents.push(agent()),
            3 => facts.agents[0].tools.push(name("missing")),
            4 => facts.agents[0].skills.push(name("missing")),
            5 => facts.agents[0].instructions = "x".repeat(registry::MAX_TEXT + 1),
            6 => facts.agents[0].model.max_output_tokens = 0,
            _ => facts.agents = vec![agent(); MAX_DEPENDENCIES + 1],
        }
        assert!(build(&source, facts).is_err(), "variant {variant}");
    }
}

#[test]
fn workflow_publication_validates_skill_requirements_without_granting_tools() {
    let mut facts = snapshots();
    facts.tools.push(tool_snapshot("tool.call"));
    facts.skills.push(SkillSnapshot {
        company_id: company(),
        name: name("support"),
        instructions: "Help".into(),
        required_tools: vec![name("lookup")],
    });
    facts.agents[0].skills.push(name("support"));
    assert!(build(&source("agent.run"), facts.clone()).is_err());
    facts.agents[0].tools.push(name("lookup"));
    assert!(build(&source("agent.run"), facts.clone()).is_ok());
    facts.profiles.push(registry::CapabilityProfile {
        name: name("broken"),
        tools: vec![],
        skills: vec![name("support")],
    });
    assert!(build(&source("agent.run"), facts).is_err());
}

#[test]
fn workflow_publication_runtime_agent_selection_uses_only_frozen_catalogue() {
    let mut source = source("agent.run");
    source["parameter_schema"] =
        json!({"type":"object","required":["agent"],"properties":{"agent":{"type":"string"}}});
    source["steps"]["start"]["with"]["agent"] = json!({"ref":"/params/agent"});
    let bundle = build(&source, snapshots()).unwrap();
    let outputs = BTreeMap::new();
    for selected in ["assistant", "unpublished"] {
        let params = json!({"agent":selected});
        let context = Context {
            input: &Value::Null,
            params: &params,
            step_outputs: &outputs,
            run: RunMetadata {
                run_id: RunId::new(Uuid::nil()),
                parent_run_id: None,
            },
        };
        let result = bundle.prepare_step_inputs(&StepId::parse("start").unwrap(), &context);
        assert_eq!(result.is_ok(), selected == "assistant");
        if let Err(error) = result {
            assert_eq!(&*error.field_path, "/steps/start/with/agent");
            assert!(error.span.line > 0);
        }
    }
}

#[test]
fn workflow_publication_mcp_contract_policy_and_resource_are_frozen_together() {
    let source = source("mcp.call");
    let facts = DependencySnapshots {
        tools: vec![tool_snapshot("mcp.call")],
        ..Default::default()
    };
    let bundle = build(&source, facts.clone()).unwrap();
    assert_eq!(bundle.manifest()["resources"][0]["kind"], "mcp");
    assert_eq!(
        bundle.manifest()["dependencies"]["tools"][0]["policy"]["recovery"],
        "safe_repeat"
    );
    for variant in 0..5 {
        let mut changed = facts.clone();
        match variant {
            0 => changed.tools[0].company_id = CompanyId::new(Uuid::nil()),
            1 => changed.tools[0].policy.policy_revision = 0,
            2 => {
                changed.tools[0].contract.connection = Some(ResourceName::parse("missing").unwrap())
            }
            3 => changed.tools[0].contract.name = name("missing"),
            _ => changed.tools.push(changed.tools[0].clone()),
        }
        assert!(build(&source, changed).is_err());
    }
    let mut later = facts;
    later.tools[0].policy.recovery = ActionRecovery::Reconcile;
    later.tools[0].policy.effect = ActionEffect::Write;
    later.tools[0].contract.output_schema = json!({"type":"object"});
    assert_ne!(
        bundle.content_hash(),
        build(&source, later).unwrap().content_hash()
    );
}

fn child_source() -> Value {
    let mut child = source("data.map");
    child["workflow_id"] = json!(Uuid::from_u128(2));
    child
}
fn child(source: &Value) -> Arc<PublishedBundle> {
    Arc::new(
        freeze(
            decode(&source.to_string()).unwrap(),
            company(),
            VersionId::new(Uuid::from_u128(3)),
            DependencySnapshots::default(),
            vec![],
        )
        .unwrap(),
    )
}
fn parent(
    source: &Value,
    children: Vec<Arc<PublishedBundle>>,
) -> Result<PublishedBundle, Diagnostic> {
    freeze(
        decode(&source.to_string())?,
        company(),
        VersionId::new(Uuid::from_u128(10)),
        DependencySnapshots::default(),
        children,
    )
}

#[test]
fn workflow_publication_children_pin_content_schemas_and_transitive_hashes() {
    let source = source("workflow.call");
    let child = child(&child_source());
    let bundle = parent(&source, vec![child.clone()]).unwrap();
    let old_hash = bundle.content_hash().clone();
    let mut edited = child_source();
    edited["limits"]["max_steps"] = json!(99);
    let later = super::tests::child(&edited);
    assert_ne!(child.content_hash(), later.content_hash());
    assert_ne!(
        bundle.content_hash(),
        parent(&source, vec![later]).unwrap().content_hash()
    );
    assert_eq!(bundle.content_hash(), &old_hash);
    assert!(Arc::ptr_eq(
        &bundle.children()[&VersionId::new(Uuid::from_u128(3))],
        &child
    ));
}

#[test]
fn workflow_publication_rejects_missing_duplicate_foreign_recursive_and_wrong_child_versions() {
    let source = source("workflow.call");
    let child = child(&child_source());
    assert!(parent(&source, vec![]).is_err());
    assert!(parent(&source, vec![child.clone(), child.clone()]).is_err());
    let foreign = Arc::new(
        freeze(
            decode(&child_source().to_string()).unwrap(),
            CompanyId::new(Uuid::nil()),
            VersionId::new(Uuid::from_u128(3)),
            DependencySnapshots::default(),
            vec![],
        )
        .unwrap(),
    );
    assert!(parent(&source, vec![foreign]).is_err());
    let mut wrong = source.clone();
    wrong["steps"]["start"]["child_version_id"] = json!(Uuid::from_u128(99));
    assert!(parent(&wrong, vec![child.clone()]).is_err());
    wrong = source.clone();
    wrong["steps"]["start"]["child_workflow_id"] = json!(Uuid::from_u128(99));
    assert!(parent(&wrong, vec![child.clone()]).is_err());
    wrong = source;
    wrong["workflow_id"] = json!(Uuid::from_u128(2));
    assert!(parent(&wrong, vec![child]).is_err());
}

#[test]
fn workflow_publication_rejects_deep_and_aggregate_schema_facts_before_copying() {
    let source = source("mcp.call");
    let mut facts = DependencySnapshots {
        tools: vec![tool_snapshot("mcp.call")],
        ..Default::default()
    };
    let mut schema = json!(true);
    for _ in 0..70 {
        schema = json!({"items":schema});
    }
    facts.tools[0].contract.input_schema = schema;
    assert!(build(&source, facts.clone()).is_err());
    facts.tools[0].contract.input_schema = json!({"description":"x".repeat(600_000)});
    facts.tools[0].contract.output_schema = json!({"description":"x".repeat(600_000)});
    assert!(build(&source, facts).is_err());
}

#[test]
fn workflow_publication_bounds_child_depth_and_pins_transitive_content() {
    let leaf = child(&child_source());
    let mut nested = leaf;
    for index in 1..=MAX_DEPENDENCY_DEPTH {
        let mut source = source("workflow.call");
        source["workflow_id"] = json!(Uuid::from_u128(1000 + index as u128));
        let definition = nested.compiled().graph().definition();
        source["steps"]["start"]["child_workflow_id"] = json!(definition.workflow_id);
        source["steps"]["start"]["child_version_id"] = json!(definition.version_id);
        let built = freeze(
            decode(&source.to_string()).unwrap(),
            company(),
            VersionId::new(Uuid::from_u128(2000 + index as u128)),
            DependencySnapshots::default(),
            vec![nested],
        );
        if index == MAX_DEPENDENCY_DEPTH {
            assert!(built.is_err());
            return;
        }
        nested = Arc::new(built.unwrap());
    }
}

#[test]
fn workflow_publication_rejects_conflicting_transitive_version_content() {
    let old = child(&child_source());
    let mut changed = child_source();
    changed["limits"]["max_steps"] = json!(99);
    let new = child(&changed);
    let mut intermediary = source("workflow.call");
    intermediary["workflow_id"] = json!(Uuid::from_u128(4));
    let intermediary = Arc::new(
        freeze(
            decode(&intermediary.to_string()).unwrap(),
            company(),
            VersionId::new(Uuid::from_u128(5)),
            DependencySnapshots::default(),
            vec![new],
        )
        .unwrap(),
    );
    let mut root = source("workflow.call");
    root["steps"]["second"] = root["steps"]["start"].clone();
    root["steps"]["start"]["routes"]["success"] = json!("second");
    root["steps"]["second"]["child_workflow_id"] = json!(Uuid::from_u128(4));
    root["steps"]["second"]["child_version_id"] = json!(Uuid::from_u128(5));
    assert!(parent(&root, vec![old, intermediary]).is_err());
}

#[test]
fn workflow_publication_inline_profiles_check_static_and_runtime_dependencies() {
    let mut facts = snapshots();
    facts.tools.push(tool_snapshot("tool.call"));
    facts.skills.push(SkillSnapshot {
        company_id: company(),
        name: name("support"),
        instructions: "Help".into(),
        required_tools: vec![name("lookup")],
    });
    let cases = [
        (json!({"tools":["missing"],"skills":[]}), "Tool missing"),
        (json!({"tools":[],"skills":["missing"]}), "Skill missing"),
        (
            json!({"tools":[],"skills":["support"]}),
            "requires unselected tool lookup",
        ),
    ];
    for (profile, reason) in cases {
        let mut source = source("agent.run");
        source["steps"]["start"]["with"]["capability_profile"] = json!({"literal":profile});
        let error = build(&source, facts.clone()).err().unwrap();
        assert_eq!(error.code, "publication.selection");
        assert_eq!(&*error.field_path, "/steps/start/with/capability_profile");
        assert!(error.message.contains(reason));
        source["parameter_schema"] = json!({"type":"object","required":["profile"],"properties":{"profile":{"type":"object"}}});
        source["steps"]["start"]["with"]["capability_profile"] = json!({"ref":"/params/profile"});
        let bundle = build(&source, facts.clone()).unwrap();
        let params = json!({"profile":profile});
        let outputs = BTreeMap::new();
        let context = Context {
            input: &Value::Null,
            params: &params,
            step_outputs: &outputs,
            run: RunMetadata {
                run_id: RunId::new(Uuid::nil()),
                parent_run_id: None,
            },
        };
        let error = bundle
            .prepare_step_inputs(&StepId::parse("start").unwrap(), &context)
            .err()
            .unwrap();
        assert_eq!(error.code, "publication.selection");
        assert!(error.message.contains(reason));
    }
    let mut source = source("agent.run");
    source["steps"]["start"]["with"]["capability_profile"] =
        json!({"object":{"tools":{"literal":["lookup"]},"skills":{"literal":["support"]}}});
    build(&source, facts).unwrap();
}

#[test]
fn workflow_publication_dependency_errors_name_the_problem_and_source_field() {
    let mut facts = snapshots();
    facts.agents[0].company_id = CompanyId::new(Uuid::nil());
    let error = build(&source("agent.run"), facts).err().unwrap();
    assert_eq!(error.code, "publication.ownership");
    assert_eq!(&*error.field_path, "/steps/start/with/agent");
    assert!(
        error
            .message
            .contains("assistant: belongs to another company")
    );
    let error = parent(&source("workflow.call"), vec![]).err().unwrap();
    assert_eq!(error.code, "publication.missing");
    assert_eq!(&*error.field_path, "/steps/start/child_version_id");
    assert!(error.message.contains(&Uuid::from_u128(3).to_string()));
    let pinned = child(&child_source());
    let error = parent(&source("workflow.call"), vec![pinned.clone(), pinned])
        .err()
        .unwrap();
    assert_eq!(error.code, "publication.duplicate");
    assert!(error.message.contains("more than once"));
    let mut facts = DependencySnapshots {
        tools: vec![tool_snapshot("mcp.call")],
        ..Default::default()
    };
    facts.tools[0].policy.policy_revision = 0;
    let error = build(&source("mcp.call"), facts).err().unwrap();
    assert_eq!(error.code, "publication.policy");
    assert_eq!(&*error.field_path, "/steps/start/with/tool");
    assert!(error.message.contains("lookup"));
}

#[test]
fn workflow_publication_inline_ambiguous_tools_fail_static_and_dynamic() {
    let mut facts = snapshots();
    facts.tools = vec![tool_snapshot("tool.call"), tool_snapshot("mcp.call")];
    let mut source = source("agent.run");
    source["resources"] = json!([{"slot":"service","kind":"mcp"}]);
    let profile = json!({"tools":["lookup"],"skills":[]});
    source["steps"]["start"]["with"]["capability_profile"] = json!({"literal":profile});
    assert_eq!(
        build(&source, facts.clone()).err().unwrap().code,
        "publication.selection"
    );
    source["parameter_schema"] =
        json!({"type":"object","required":["profile"],"properties":{"profile":{"type":"object"}}});
    source["steps"]["start"]["with"]["capability_profile"] = json!({"ref":"/params/profile"});
    let bundle = build(&source, facts).unwrap();
    let params = json!({"profile":profile});
    let outputs = BTreeMap::new();
    let context = Context {
        input: &Value::Null,
        params: &params,
        step_outputs: &outputs,
        run: RunMetadata {
            run_id: RunId::new(Uuid::nil()),
            parent_run_id: None,
        },
    };
    let error = bundle
        .prepare_step_inputs(&StepId::parse("start").unwrap(), &context)
        .err()
        .unwrap();
    assert_eq!(error.code, "publication.selection");
    assert!(error.message.contains("lookup is missing or ambiguous"));
}
