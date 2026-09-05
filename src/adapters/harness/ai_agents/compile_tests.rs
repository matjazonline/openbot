//! The compiler's own tests. Pure: no runtime, no provider, no mocks.
//!
//! These carry the safety properties of the whole feature. The allowlist is the platform's only
//! enforcement point for what an agent may call, and the round-trip below is the only thing that
//! catches an emitted key the runtime would refuse -- `SkillDefinition` and `SkillStep` are both
//! `deny_unknown_fields`, so one stray field is a hard parse failure at run time rather than an
//! ignored extra.

use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use super::*;
use crate::entities::{
    creation::CreationProvenance,
    harness::{HarnessConfig, HarnessKind, SubAgentScope},
    skill::{Skill, SkillInstruction},
    tool_catalogue::{ALLOWED_BUILTIN_TOOL_IDS, OUTREACH_TOOL_ID},
    value_objects::{ModelName, ModelProvider, SkillSlug},
};
use crate::services::{
    agent_directory_tool::ListCompanyAgentsTool,
    agent_runner::{DEFAULT_AGENT_NAME, DEFAULT_SYSTEM_PROMPT},
    outreach_tool::OutreachAndAwaitQuorumTool,
};

/// Every built-in the pinned revision ships, so the exclusions below can be named rather than
/// inferred from a list this codebase also writes.
const UPSTREAM_BUILTIN_TOOL_IDS: [&str; 30] = [
    "calculator",
    "echo",
    "datetime",
    "json",
    "random",
    "file",
    "glob",
    "grep",
    "file_read",
    "file_write",
    "file_edit",
    "patch",
    "copy_path",
    "move_path",
    "delete_path",
    "file_list",
    "file_info",
    "git_status",
    "git_diff",
    "diagnostics",
    "ask_user",
    "todo",
    "sleep",
    "web_fetch",
    "web_search",
    "command",
    "text",
    "template",
    "math",
    "http",
];

fn spec_with(extra_config: serde_json::Value) -> AgentCapabilitySpec {
    let harness_config = if extra_config == json!({}) {
        HarnessConfig::empty(HarnessKind::AiAgents)
    } else {
        HarnessConfig::parse(HarnessKind::AiAgents, Some(&extra_config))
            .expect("test config must use the reviewed schema")
    };
    AgentCapabilitySpec {
        harness: HarnessKind::AiAgents,
        name: "Support".to_string(),
        system_prompt: "You are a helpful email agent.".to_string(),
        provider: ModelProvider::canonical("openai"),
        model: ModelName::canonical("gpt-4o"),
        provider_base_url: None,
        skills: Vec::new(),
        granted_tools: Vec::new(),
        sub_agents: SubAgentScope::AllCompanySiblings,
        harness_config,
    }
}

fn skill_of(slug: &str, instructions: Vec<SkillInstruction>) -> Skill {
    Skill {
        id: Uuid::new_v4(),
        company_id: Some(Uuid::new_v4()),
        slug: SkillSlug::parse(slug).expect("a well-formed slug"),
        name: "Read the clock".to_string(),
        description: "Say what time it is.".to_string(),
        trigger: "The sender asks about timing.".to_string(),
        instructions,
        created_by: CreationProvenance::system(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn prompt_step(text: &str) -> SkillInstruction {
    SkillInstruction::Prompt {
        text: text.to_string(),
    }
}

fn tool_step(tool: &str) -> SkillInstruction {
    SkillInstruction::Tool {
        tool: ToolId::from(tool),
        args: None,
        output_as: None,
    }
}

/// The compiled configuration, as YAML parsed back into a document. Assertions are made on this
/// rather than on the string, so formatting is never what a test is really checking.
fn compiled(spec: &AgentCapabilitySpec) -> serde_yaml::Value {
    let output = compile(spec, "sk-test-123", &[]).expect("the spec compiles");
    serde_yaml::from_str(&output.yaml).expect("the compiled configuration is YAML")
}

fn granted_tools(document: &serde_yaml::Value) -> Vec<String> {
    document["tools"]
        .as_sequence()
        .map(|tools| {
            tools
                .iter()
                .map(|tool| tool.as_str().expect("a simple tool entry").to_string())
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------
// The allowlist
// ---------------------------------------------------------------------------------------------

#[test]
fn command_is_dropped_from_the_grant() {
    let mut spec = spec_with(json!({}));
    spec.granted_tools = vec![ToolId::from("command"), ToolId::from("calculator")];

    let output = compile(&spec, "sk-test-123", &[]).expect("the spec compiles");
    let document: serde_yaml::Value =
        serde_yaml::from_str(&output.yaml).expect("the compiled configuration is YAML");

    assert_eq!(granted_tools(&document), ["calculator"]);
    assert_eq!(output.refused_tools, [ToolId::from("command")]);
}

/// The twenty built-ins that reach or depend on this host: arbitrary execution, the file families,
/// the repository pair, diagnostics, an unbounded sleep inside a leased task, unrestricted HTTP,
/// host-dependent web search, and a prompt for a terminal operator who does not exist here.
#[test]
fn every_host_access_builtin_is_dropped() {
    let excluded: Vec<&str> = UPSTREAM_BUILTIN_TOOL_IDS
        .into_iter()
        .filter(|id| !ALLOWED_BUILTIN_TOOL_IDS.contains(id))
        .collect();
    assert_eq!(
        excluded.len(),
        20,
        "the allowlist admits 10 of 30 built-ins"
    );

    for id in excluded {
        let mut spec = spec_with(json!({}));
        spec.granted_tools = vec![ToolId::from(id)];

        let output = compile(&spec, "sk-test-123", &[]).expect("the spec compiles");
        let document: serde_yaml::Value =
            serde_yaml::from_str(&output.yaml).expect("the compiled configuration is YAML");

        assert!(granted_tools(&document).is_empty(), "{id} was granted");
        assert_eq!(output.refused_tools, [ToolId::from(id)], "{id}");
    }
}

/// A native tool is grantable in general and servable only by a run that was given its context.
/// A grant this run cannot serve is dropped and reported separately -- offering the model a tool
/// that fails on use is worse than not offering it.
#[test]
fn a_native_grant_this_run_cannot_serve_is_dropped_separately_from_a_refused_one() {
    let mut spec = spec_with(json!({}));
    spec.granted_tools = vec![ToolId::from(OUTREACH_TOOL_ID), ToolId::from("datetime")];

    let without = compile(&spec, "sk-test-123", &[]).expect("the spec compiles");
    assert!(without.refused_tools.is_empty());
    assert_eq!(without.unavailable_tools, [ToolId::from(OUTREACH_TOOL_ID)]);

    let offered = [OutreachAndAwaitQuorumTool::declaration()];
    let with = compile(&spec, "sk-test-123", &offered).expect("the spec compiles");
    let document: serde_yaml::Value =
        serde_yaml::from_str(&with.yaml).expect("the compiled configuration is YAML");
    assert!(with.unavailable_tools.is_empty());
    assert_eq!(granted_tools(&document), [OUTREACH_TOOL_ID, "datetime"]);
}

/// The `tools:` list is the only grant, and a skill's tool step is checked against it exactly
/// like a model-initiated call. Forgetting this union is not a compile error -- it is a skill
/// that dies mid-run on "not available in the current scope".
#[test]
fn a_skills_referenced_tools_are_unioned_into_the_grant() {
    let mut spec = spec_with(json!({}));
    spec.granted_tools = vec![ToolId::from("calculator")];
    spec.skills = vec![skill_of(
        "read-the-clock",
        vec![
            tool_step("datetime"),
            tool_step("json"),
            prompt_step("Say what time it is."),
        ],
    )];

    let document = compiled(&spec);

    assert_eq!(
        granted_tools(&document),
        ["calculator", "datetime", "json"],
        "grants first, then each skill's tools in first-use order"
    );
}

/// A skill step naming an ungrantable tool. `Skill::validate` refuses such a skill at write time,
/// so this can only be a row that predates the rule or one inserted around the use case. The
/// decision, made here rather than left undefined: the skill still compiles, the tool is reported
/// as refused, and the runtime denies that one step -- the rest of the skill still runs.
#[test]
fn a_skill_referencing_an_ungrantable_tool_loses_the_grant_and_is_reported() {
    let mut spec = spec_with(json!({}));
    spec.skills = vec![skill_of(
        "run-a-script",
        vec![tool_step("command"), prompt_step("Report what happened.")],
    )];

    let output = compile(&spec, "sk-test-123", &[]).expect("the spec compiles");
    let document: serde_yaml::Value =
        serde_yaml::from_str(&output.yaml).expect("the compiled configuration is YAML");

    assert_eq!(output.refused_tools, [ToolId::from("command")]);
    assert!(granted_tools(&document).is_empty());
    assert_eq!(
        document["skills"]
            .as_sequence()
            .expect("the skill still compiles")
            .len(),
        1
    );
}

/// The registry `auto_configure_features()` installs. If the pinned revision renames or drops a
/// built-in, this fails here rather than silently dropping the grant at run time -- the runtime
/// would otherwise deny the call with no build-time signal at all.
#[test]
fn every_allowlisted_id_exists_upstream() {
    let registry = ai_agents::tools::create_builtin_registry();
    let ids = registry.list_ids();

    for id in ALLOWED_BUILTIN_TOOL_IDS {
        assert!(
            ids.iter().any(|known| known == id),
            "{id} is no longer an ai-agents builtin"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Skills
// ---------------------------------------------------------------------------------------------

/// The important one. `SkillDefinition` and `SkillStep` are `deny_unknown_fields`, so this
/// round-trip is what catches an extra emitted key -- and it asserts on the parsed spec rather
/// than on the YAML string, which would only be checking formatting.
#[test]
fn the_compiled_yaml_deserializes_into_ai_agents_agent_spec() {
    let mut spec = spec_with(json!({}));
    spec.granted_tools = vec![ToolId::from("datetime")];
    spec.skills = vec![skill_of(
        "read-the-clock",
        vec![
            SkillInstruction::Tool {
                tool: ToolId::from("datetime"),
                args: Some(json!({ "operation": "now" })),
                output_as: Some("now".to_string()),
            },
            prompt_step("Tell the sender it is {{ now }}."),
        ],
    )];
    let output = compile(&spec, "sk-test-123", &[]).expect("the spec compiles");

    let parsed = ai_agents::AgentSpec::from_yaml_strict(&output.yaml)
        .expect("the runtime accepts what we compiled");

    assert_eq!(parsed.name, "Support");
    let ai_agents::SkillRef::Inline(skill) = &parsed.skills[0] else {
        panic!("skills are emitted inline, not by name or file");
    };
    assert_eq!(skill.id, "read-the-clock");
    assert_eq!(skill.description, "Say what time it is.");
    assert_eq!(skill.trigger, "The sender asks about timing.");
    assert_eq!(skill.steps.len(), 2);
    assert!(skill.reasoning.is_none());
    assert!(skill.reflection.is_none());
    assert!(skill.disambiguation.is_none());

    match &skill.steps[0] {
        ai_agents::SkillStep::Tool {
            tool,
            args,
            output_as,
        } => {
            assert_eq!(tool, "datetime");
            assert_eq!(args.as_ref(), Some(&json!({ "operation": "now" })));
            assert_eq!(output_as.as_deref(), Some("now"));
        }
        other => panic!("expected a tool step, got {other:?}"),
    }
    match &skill.steps[1] {
        ai_agents::SkillStep::Prompt { prompt, llm } => {
            assert_eq!(prompt, "Tell the sender it is {{ now }}.");
            assert!(llm.is_none(), "one provider is registered per run");
        }
        other => panic!("expected a prompt step, got {other:?}"),
    }
}

/// `SkillStep` is untagged, so the key name is the discriminator. A map carrying both keys, or
/// neither, deserializes as an unhelpful untagged error.
#[test]
fn a_prompt_only_skill_emits_no_tool_key_and_vice_versa() {
    let mut spec = spec_with(json!({}));
    spec.skills = vec![skill_of(
        "just-answer",
        vec![tool_step("datetime"), prompt_step("Answer.")],
    )];

    let document = compiled(&spec);
    let steps = document["skills"][0]["steps"]
        .as_sequence()
        .expect("the skill has steps");

    assert!(steps[0].get("tool").is_some());
    assert!(steps[0].get("prompt").is_none());
    // Both optionals are omitted rather than emitted as null: `deny_unknown_fields` accepts the
    // key, but a null `args` is not the same document as no `args`.
    assert!(steps[0].get("args").is_none());
    assert!(steps[0].get("output_as").is_none());

    assert!(steps[1].get("prompt").is_some());
    assert!(steps[1].get("tool").is_none());
    assert!(steps[1].get("llm").is_none());
}

/// An agent with no skills of its own gains no `skills:` key at all. This phase must not change
/// a single existing agent's configuration.
#[test]
fn an_agent_with_no_skills_compiles_to_a_document_without_the_key() {
    let document = compiled(&spec_with(json!({})));

    assert!(document.get("skills").is_none());
}

/// An agent that grants nothing compiles to the document it compiled to before there was a grant
/// list at all -- no empty `tools:`, and so no `tool_choice` either.
#[test]
fn a_grant_that_survives_nothing_leaves_no_tools_key_behind() {
    let mut spec = spec_with(json!({}));
    spec.granted_tools = vec![ToolId::from("file_write")];

    let document = compiled(&spec);

    assert!(document.get("tools").is_none());
    assert!(document["llm"].get("tool_choice").is_none());
}

// ---------------------------------------------------------------------------------------------
// The rest of the compiled document
// ---------------------------------------------------------------------------------------------

#[test]
fn the_specs_name_and_prompt_are_server_owned() {
    let plain = compiled(&spec_with(json!({})));
    assert_eq!(plain["name"].as_str(), Some("Support"));
    assert!(
        plain["system_prompt"]
            .as_str()
            .expect("a system prompt")
            .starts_with("You are a helpful email agent.")
    );
}

#[test]
fn the_credential_and_model_selection_are_stamped_by_the_server() {
    let document = compiled(&spec_with(json!({})));

    assert_eq!(document["llm"]["provider"].as_str(), Some("openai"));
    assert_eq!(document["llm"]["model"].as_str(), Some("gpt-4o"));
    assert_eq!(document["llm"]["api_key"].as_str(), Some("sk-test-123"));
}

#[test]
fn tool_choice_defaults_to_auto_when_the_grant_is_non_empty() {
    let mut config = json!({ "tools": [{ "name": OUTREACH_TOOL_ID }] });

    ensure_config_fields(
        &mut config,
        "openai",
        "gpt-4o",
        "sk-test-123",
        DEFAULT_SYSTEM_PROMPT,
        DEFAULT_AGENT_NAME,
    );

    assert_eq!(config["llm"]["tool_choice"].as_str(), Some("auto"));
}

#[test]
fn an_explicit_tool_choice_survives_including_one_that_turns_tools_off() {
    for explicit in ["required", "none"] {
        let mut config = json!({
            "tools": [{ "name": OUTREACH_TOOL_ID }],
            "llm": { "tool_choice": explicit },
        });

        ensure_config_fields(
            &mut config,
            "openai",
            "gpt-4o",
            "sk-test-123",
            DEFAULT_SYSTEM_PROMPT,
            DEFAULT_AGENT_NAME,
        );

        assert_eq!(
            config["llm"]["tool_choice"].as_str(),
            Some(explicit),
            "an operator who named a choice must keep it"
        );
    }
}

/// No grant, no choice. The default exists to make a grant effective, not to advertise a tool
/// protocol to an agent that has no tools.
#[test]
fn an_agent_without_tools_is_given_no_tool_choice() {
    for empty in [
        json!({}),
        json!({ "tools": [] }),
        // Policy for a tool is not a grant of it: `declared_tool_ids` is built from `tools:`.
        json!({ "tool_security": { "tools": { OUTREACH_TOOL_ID: {} } } }),
    ] {
        let mut config = empty.clone();
        ensure_config_fields(
            &mut config,
            "openai",
            "gpt-4o",
            "sk-test-123",
            DEFAULT_SYSTEM_PROMPT,
            DEFAULT_AGENT_NAME,
        );
        assert!(
            config["llm"].get("tool_choice").is_none(),
            "{empty} must not gain a tool choice"
        );
    }
}

#[test]
fn ensure_config_fields_populates_missing_keys() {
    let mut config = json!({ "temperature": 0.5 });

    ensure_config_fields(
        &mut config,
        "openai",
        "gpt-4o",
        "sk-test-123",
        "Custom prompt",
        "Custom Agent",
    );

    assert_eq!(config["name"].as_str(), Some("Custom Agent"));
    assert_eq!(config["system_prompt"].as_str(), Some("Custom prompt"));
    assert_eq!(config["llm"]["provider"].as_str(), Some("openai"));
    assert_eq!(config["llm"]["model"].as_str(), Some("gpt-4o"));
    assert_eq!(config["llm"]["api_key"].as_str(), Some("sk-test-123"));
    // The SDK's own default of 2,048 truncated long replies mid-sentence.
    assert_eq!(config["llm"]["max_tokens"].as_u64(), Some(8192));
    assert_eq!(config["temperature"].as_f64(), Some(0.5));
}

#[test]
fn ensure_config_fields_preserves_an_explicit_max_tokens() {
    let mut config = json!({ "llm": { "max_tokens": 4096 } });

    ensure_config_fields(
        &mut config,
        "openai",
        "gpt-4o",
        "sk-test-123",
        DEFAULT_SYSTEM_PROMPT,
        DEFAULT_AGENT_NAME,
    );

    assert_eq!(config["llm"]["max_tokens"].as_u64(), Some(4096));
}

#[test]
fn ensure_config_fields_preserves_an_existing_name_and_system_prompt() {
    let mut config = json!({
        "name": "CustomAgent",
        "system_prompt": "You are a custom assistant.",
    });

    ensure_config_fields(
        &mut config,
        "openai",
        "gpt-4o",
        "sk-test-123",
        DEFAULT_SYSTEM_PROMPT,
        DEFAULT_AGENT_NAME,
    );
    assert_eq!(config["name"], "CustomAgent");
    assert_eq!(config["system_prompt"], "You are a custom assistant.");

    // Even with fallbacks supplied, an existing value takes precedence.
    ensure_config_fields(
        &mut config,
        "openai",
        "gpt-4o",
        "sk-test-123",
        "Fallback prompt",
        "Fallback name",
    );
    assert_eq!(config["name"], "CustomAgent");
    assert_eq!(config["system_prompt"], "You are a custom assistant.");
}

#[test]
fn appending_the_base_context_prompt_is_idempotent() {
    let mut config = json!({ "system_prompt": "Initial prompt" });

    append_base_context_prompt(&mut config);
    let once = config["system_prompt"].as_str().unwrap().to_string();
    assert!(once.contains(BASE_CONTEXT_SYSTEM_PROMPT));

    append_base_context_prompt(&mut config);
    assert_eq!(config["system_prompt"].as_str(), Some(once.as_str()));
}

#[test]
fn the_base_context_prompt_exposes_basic_runtime_information() {
    for variable in [
        "{{ context.time.date }}",
        "{{ context.time.time }}",
        "{{ context.agent_info.name }}",
        "{{ context.recipient_role }}",
        "{{ context.is_to }}",
        "{{ context.is_cc }}",
    ] {
        assert!(BASE_CONTEXT_SYSTEM_PROMPT.contains(variable));
    }
    assert!(!BASE_CONTEXT_SYSTEM_PROMPT.contains("context.session"));
}

#[test]
fn the_base_config_keeps_observability_private_and_in_memory() {
    let config = base_agent_config_with_observability(true);

    assert_eq!(config["observability"]["enabled"], true);
    for privacy in [
        "include_prompts",
        "include_responses",
        "include_tool_args",
        "include_tool_outputs",
    ] {
        assert_eq!(config["observability"]["privacy"][privacy], false);
    }
    assert_eq!(config["observability"]["export"]["write_report"], false);
    assert_eq!(config["observability"]["export"]["write_raw_events"], false);
}

#[test]
fn the_base_config_declares_the_delivery_context() {
    let config = base_agent_config_with_observability(false);

    for key in ["recipient_role", "is_to", "is_cc"] {
        assert_eq!(config["context"][key]["type"], "runtime");
        assert_eq!(config["context"][key]["required"], true);
    }
}

/// The adapter's server-owned defaults match the native tools' defaults. Per-agent policy is not
/// represented here: it remains typed and is applied directly by the host tool implementations.
#[test]
fn the_base_tool_policy_matches_what_the_tools_default_to_on_their_own() {
    let config = base_agent_config_with_observability(false);
    let outreach = &config["tool_security"]["tools"][OUTREACH_TOOL_ID]["config"];

    assert_eq!(
        outreach["max_targets"].as_u64(),
        Some(crate::services::outreach_tool::DEFAULT_MAX_TARGETS as u64)
    );
    assert_eq!(
        outreach["default_timeout_hours"].as_u64(),
        Some(crate::services::outreach_tool::DEFAULT_TIMEOUT_HOURS as u64)
    );
    assert_eq!(
        outreach["max_timeout_hours"].as_u64(),
        Some(crate::services::outreach_tool::MAX_TIMEOUT_HOURS as u64)
    );
    assert_eq!(
        outreach["allowed_target_scope"].as_str(),
        Some(crate::services::outreach_tool::DEFAULT_ALLOWED_TARGET_SCOPE)
    );
    // Outbound mail on an agent's own authority is the thing that needs saying out loud.
    assert_eq!(outreach["internal_requires_approval"], true);

    assert_eq!(
        config["tool_security"]["tools"]
            [ListCompanyAgentsTool::declaration().id.as_str()]["config"]["max_results"]
            .as_u64(),
        Some(crate::services::agent_directory_tool::DEFAULT_DIRECTORY_MAX_RESULTS as u64)
    );
}
