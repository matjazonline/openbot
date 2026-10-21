use super::super::{
    providers::ProviderRegistry,
    skills::render_document,
    test_support::{Reply, model, response},
    tools::ToolCorrelationId,
};
use super::*;
use crate::{
    entities::{
        agent::Agent as AgentEntity,
        creation::CreationProvenance,
        harness::{HarnessConfig, HarnessKind, SubAgentScope},
        skill::{Skill, SkillInstruction},
        transport::RecipientRole,
    },
    services::{
        harness::context::RuntimeContext,
        test_support::{ScriptedExchange, ScriptedResponse, scripted_scenario},
    },
    use_cases::skill::StoredAgentCapabilities,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::sync::{
    RwLock,
    atomic::{AtomicUsize, Ordering},
};
use uuid::Uuid;

struct Clock(AtomicUsize);
impl RuntimeClock for Clock {
    fn now(&self) -> DateTime<Utc> {
        DateTime::from_timestamp(
            1_783_036_800 + 60 * self.0.fetch_add(1, Ordering::SeqCst) as i64,
            0,
        )
        .unwrap()
    }
}

struct Reader {
    facts: RuntimeContext,
    skills: RwLock<Vec<Skill>>,
    reads: AtomicUsize,
}

#[async_trait]
impl AgentCapabilityReader for Reader {
    async fn load_for_execution(
        &self,
        company_id: Uuid,
        agent_id: Uuid,
    ) -> AppResult<Option<StoredAgentCapabilities>> {
        assert_eq!(company_id, self.facts.company_id);
        assert_eq!(agent_id, self.facts.agent_id);
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(Some(StoredAgentCapabilities {
            agent: AgentEntity {
                response_contract: None,
                id: agent_id,
                company_id: Some(company_id),
                name: "Assistant".into(),
                slug: "assistant".into(),
                provider: None,
                model: None,
                run_timeout_secs: None,
                system_prompt: None,
                description: None,
                config_json: None,
                memory_enabled: false,
                memory_persistence_mode: Default::default(),
                memory_recall_mode: Default::default(),
                memory_max_results: 5,
                avatar_url: None,
                created_by: CreationProvenance::system(),
                created_at: Utc::now(),
            },
            skills: self.skills.read().unwrap().clone(),
            sub_agent_scope: SubAgentScope::AllCompanySiblings,
        }))
    }
}

fn recipe() -> Skill {
    Skill {
        id: Uuid::from_u128(3),
        company_id: Some(Uuid::from_u128(1)),
        slug: "ordered-proof".into(),
        name: "Ordered proof".into(),
        description: "Two dependent prompt steps".into(),
        trigger: "Review a record".into(),
        instructions: vec![
            SkillInstruction::Prompt {
                text: "Classify the record for {{ user_input }}.".into(),
            },
            SkillInstruction::Prompt {
                text: "Answer using the class {{ steps[0].result.label }}.".into(),
            },
        ],
        created_by: CreationProvenance::system(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn fixture() -> (AgentCapabilitySpec, Arc<Reader>, CompileContext) {
    let spec = AgentCapabilitySpec {
        response_contract: None,
        harness: HarnessKind::Rig,
        name: "Assistant".into(),
        system_prompt: "Time={{ context.time.date }}".into(),
        provider: "openai".into(),
        model: "model".into(),
        provider_base_url: None,
        skills: vec![recipe()],
        sub_agents: SubAgentScope::AllCompanySiblings,
        harness_config: HarnessConfig::empty(HarnessKind::Rig),
    };
    let facts = RuntimeContext {
        company_id: Uuid::from_u128(1),
        agent_id: Uuid::from_u128(2),
        recipient_role: RecipientRole::Cc,
        timezone: chrono_tz::Europe::Ljubljana,
    };
    let reader = Arc::new(Reader {
        facts: facts.clone(),
        skills: RwLock::new(spec.skills.clone()),
        reads: AtomicUsize::new(0),
    });
    let context = CompileContext {
        facts,
        clock: Arc::new(Clock(AtomicUsize::new(0))),
        capabilities: reader.clone(),
        token_budget: 262_144,
    };
    (spec, reader, context)
}

fn uri() -> String {
    format!(
        "skill://{}/{}/{}",
        Uuid::from_u128(1),
        Uuid::from_u128(2),
        Uuid::from_u128(3)
    )
}
fn correlation() -> ToolCorrelationId {
    ToolCorrelationId::parse("resource-call").unwrap()
}

#[test]
fn document_preserves_verbatim_steps_and_trigger() {
    let mut skill = recipe();
    skill.instructions[0] = SkillInstruction::Prompt {
        text: "Quote \"this\" text\nnext line as written.".into(),
    };
    let document = render_document(&skill).unwrap();
    let (_, encoded) = document.split_once('\n').unwrap();
    let encoded: Value = serde_json::from_str(encoded).unwrap();
    assert_eq!(encoded["trigger"], skill.trigger);
    for (index, step) in skill.instructions.iter().enumerate() {
        assert_eq!(
            encoded["steps"][index],
            json!({"index":index,"instruction":step})
        );
    }
    assert_eq!(render_document(&skill).unwrap(), document);
    assert!(document.contains("Missing variables are errors"));
}

#[test]
fn templates_reject_empty_malformed_future_missing_and_executable_constructs() {
    for text in [
        "{{ }}",
        "{{ missing }}",
        "{{ steps[1].result }}",
        "{{ steps[99].result }}",
        "{{ steps[0].args }}",
        "{{ context.x",
        "oops }}",
        "{{ context.x|default('missing') }}",
        "{% for x in xs %}",
        "{{ context.x + 1 }}",
    ] {
        let mut skill = recipe();
        skill.instructions[1] = SkillInstruction::Prompt { text: text.into() };
        assert!(render_document(&skill).is_err(), "{text}");
    }
}

#[test]
fn preflight_checks_scope_collisions_context_and_bounds() {
    let (mut spec, _, context) = fixture();
    spec.skills.push(spec.skills[0].clone());
    assert!(CompiledRun::compile(&spec, context).is_err());
    let (mut spec, _, context) = fixture();
    let mut duplicate = spec.skills[0].clone();
    duplicate.id = Uuid::new_v4();
    spec.skills.push(duplicate);
    assert!(CompiledRun::compile(&spec, context).is_err());
    let (mut spec, _, context) = fixture();
    spec.skills[0].company_id = Some(Uuid::new_v4());
    assert!(CompiledRun::compile(&spec, context).is_err());
    let (mut spec, _, context) = fixture();
    spec.system_prompt = "{{ context.missing }}".into();
    assert!(CompiledRun::compile(&spec, context).is_err());
    let (mut spec, _, context) = fixture();
    spec.skills[0].instructions = vec![
        SkillInstruction::Prompt {
            text: "a".repeat(8000)
        };
        3
    ];
    assert!(CompiledRun::compile(&spec, context).is_err());
    let (spec, _, mut context) = fixture();
    context.token_budget = 10;
    assert!(CompiledRun::compile(&spec, context).is_err());
}

#[tokio::test]
async fn reads_are_exact_scoped_rechecked_and_share_budget_without_recipe_effects() {
    let (spec, reader, context) = fixture();
    let compiled = CompiledRun::compile(&spec, context).unwrap();
    let budget = compiled.budget.clone();
    let before = budget.remaining();
    for _ in 0..2 {
        let result = compiled
            .tools
            .invoke(
                &"read_resource".into(),
                &correlation(),
                json!({"skill_uri":uri()}),
            )
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(result.render(), render_document(&spec.skills[0]).unwrap());
    }
    assert_eq!(reader.reads.load(Ordering::SeqCst), 2);
    assert!(budget.remaining() < before - 2 * render_document(&spec.skills[0]).unwrap().len());
    // Unknown model-provided bodies fail schema validation, without reaching the reader.
    assert!(
        !compiled
            .tools
            .invoke(
                &"read_resource".into(),
                &correlation(),
                json!({"skill_uri":uri(), "body":"injected"})
            )
            .await
            .unwrap()
            .success
    );
    assert_eq!(reader.reads.load(Ordering::SeqCst), 2);
    reader.skills.write().unwrap().clear();
    assert!(
        compiled
            .tools
            .invoke(
                &"read_resource".into(),
                &correlation(),
                json!({"skill_uri":uri()})
            )
            .await
            .is_err()
    );
    assert!(
        compiled
            .tools
            .invoke(
                &"echo".into(),
                &correlation(),
                json!({"message":"must not run"})
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn unknown_cross_tenant_changed_and_foreign_resources_fail_closed() {
    for bad in [
        "file:///etc/passwd".to_owned(),
        "https://example.com/skill".into(),
        uri().replace("000000000001", "000000000099"),
    ] {
        let (spec, reader, context) = fixture();
        let catalog = SkillCatalog::compile(&spec, &context.facts, reader.clone()).unwrap();
        assert!(catalog.read(&bad).await.is_err());
        assert_eq!(reader.reads.load(Ordering::SeqCst), 0);
    }
    for foreign in [false, true] {
        let (spec, reader, context) = fixture();
        let catalog = SkillCatalog::compile(&spec, &context.facts, reader.clone()).unwrap();
        {
            let mut skills = reader.skills.write().unwrap();
            if foreign {
                skills[0].company_id = Some(Uuid::new_v4());
            } else {
                skills[0].trigger = "changed".into();
            }
        }
        assert!(catalog.read(&uri()).await.is_err());
    }
}

#[path = "skills_provider_tests.rs"]
mod provider;

#[tokio::test]
async fn repeated_loads_exhaust_the_same_budget_and_latch_dispatch() {
    let (spec, _, context) = fixture();
    let compiled = CompiledRun::compile(&spec, context).unwrap();
    compiled
        .budget
        .charge(compiled.budget.remaining() - 100)
        .unwrap();
    assert!(
        compiled
            .tools
            .invoke(
                &"read_resource".into(),
                &correlation(),
                json!({"skill_uri":uri()})
            )
            .await
            .is_err()
    );
    assert_eq!(
        compiled.tools.stop_reason().await,
        Some(ToolStopReason::Budget)
    );
    assert!(
        compiled
            .tools
            .invoke(&"echo".into(), &correlation(), json!({"message":"x"}))
            .await
            .is_err()
    );
}
