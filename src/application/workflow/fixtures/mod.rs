//! Illustrative sources and captured facts for handler tests. The revision pair is the shipped
//! support template (07-D1 §D1.8); production catalogue wiring belongs to phases 08 and 09.
//! None of these is a company grant.

use super::publication::{
    AgentReference, AgentReferenceUse, AgentUseMode, DependencySnapshots, ToolSnapshot,
};
use super::registry::ToolContract;
use crate::application::agent::{ActionEffect, ActionRecovery, AgentKey, ApprovedActionPolicy};
use crate::domain::workflow::{CompanyId, ResourceName, TypeName, VersionId};
use serde_json::json;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fixture {
    ReviewedSupport,
    AutonomousResponse,
    TriageRouting,
    RepeatedHumanRevision,
    HumanRevisionRound,
    McpLookup,
    ReviewFeedbackSuccessor,
}

pub const ALL: [Fixture; 7] = [
    Fixture::ReviewedSupport,
    Fixture::AutonomousResponse,
    Fixture::TriageRouting,
    Fixture::RepeatedHumanRevision,
    Fixture::HumanRevisionRound,
    Fixture::McpLookup,
    Fixture::ReviewFeedbackSuccessor,
];

impl Fixture {
    pub fn source(self) -> &'static str {
        match self {
            Self::ReviewedSupport => include_str!("reviewed-support.yaml"),
            Self::AutonomousResponse => include_str!("autonomous-response.yaml"),
            Self::TriageRouting => include_str!("triage-routing.yaml"),
            Self::RepeatedHumanRevision => include_str!("repeated-human-revision.yaml"),
            Self::HumanRevisionRound => include_str!("human-revision-round.yaml"),
            Self::McpLookup => include_str!("mcp-lookup.yaml"),
            Self::ReviewFeedbackSuccessor => include_str!("review-feedback-successor.yaml"),
        }
    }

    pub fn version_id(self) -> VersionId {
        let suffix = match self {
            Self::ReviewedSupport => 101,
            Self::AutonomousResponse => 102,
            Self::TriageRouting => 103,
            Self::RepeatedHumanRevision => 104,
            Self::HumanRevisionRound => 105,
            Self::McpLookup => 106,
            Self::ReviewFeedbackSuccessor => 107,
        };
        VersionId::new(Uuid::from_u128(suffix))
    }

    pub fn child(self) -> Option<Self> {
        (self == Self::RepeatedHumanRevision).then_some(Self::HumanRevisionRound)
    }

    /// Caller chooses the test tenant. These synthetic facts never confer authority.
    pub fn snapshots(self, company: CompanyId) -> DependencySnapshots {
        match self {
            Self::ReviewedSupport
            | Self::AutonomousResponse
            | Self::RepeatedHumanRevision
            | Self::HumanRevisionRound
            | Self::ReviewFeedbackSuccessor
            | Self::TriageRouting => agent_snapshots(self, company),
            Self::McpLookup => DependencySnapshots {
                tools: vec![mcp_snapshot(company)],
                ..Default::default()
            },
        }
    }
}

fn name(value: &str) -> TypeName {
    TypeName::parse(value).expect("static fixture name")
}

fn agent_snapshots(fixture: Fixture, company: CompanyId) -> DependencySnapshots {
    let steps: &[(&str, AgentUseMode)] = match fixture {
        Fixture::ReviewedSupport => &[("draft", AgentUseMode::Tools)],
        Fixture::AutonomousResponse => &[("draft", AgentUseMode::Tools)],
        Fixture::TriageRouting => &[("classify", AgentUseMode::ModelOnly)],
        Fixture::HumanRevisionRound => &[("draft", AgentUseMode::Tools)],
        Fixture::ReviewFeedbackSuccessor => &[
            ("draft", AgentUseMode::Tools),
            ("specialist", AgentUseMode::Tools),
        ],
        Fixture::RepeatedHumanRevision => &[("specialist", AgentUseMode::Tools)],
        Fixture::McpLookup => &[],
    };
    DependencySnapshots {
        agents: if steps.is_empty() {
            vec![]
        } else {
            vec![AgentReference {
                agent_id: crate::entities::agent_capability::AgentId::new(uuid::Uuid::from_u128(1)),
                company_id: company,
                key: AgentKey::parse("assistant").expect("fixture agent key"),
                uses: steps
                    .iter()
                    .map(|(step, mode)| AgentReferenceUse {
                        step: crate::domain::workflow::StepId::parse(*step).expect("fixture step"),
                        mode: *mode,
                    })
                    .collect(),
            }]
        },
        tools: vec![ToolSnapshot {
            workflow: None,
            company_id: company,
            contract: ToolContract {
                name: name("knowledge.search"),
                connection: None,
                input_schema: json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"],"additionalProperties":false}),
                output_schema: json!({"type":"array","items":{"type":"string"},"maxItems":10}),
            },
            policy: read_policy(),
        }],
    }
}

fn read_policy() -> ApprovedActionPolicy {
    ApprovedActionPolicy {
        capability: name("fixture.read"),
        policy_revision: 1,
        effect: ActionEffect::Read,
        recovery: ActionRecovery::SafeRepeat,
    }
}

fn mcp_snapshot(company: CompanyId) -> ToolSnapshot {
    ToolSnapshot {
        workflow: None,
        company_id: company,
        contract: ToolContract {
            name: name("lookup_tickets"),
            connection: Some(ResourceName::parse("tickets").expect("static fixture slot")),
            input_schema: json!({"type":"object","properties":{
                "limit":{"type":"integer","minimum":1,"maximum":20},
                "include_closed":{"type":"boolean"},
                "labels":{"type":"array","items":{"type":"string"}},
                "filter":{"type":"object","properties":{"owner":{"type":["string","null"]}},"required":["owner"],"additionalProperties":false}
            },"required":["limit","include_closed","labels","filter"],"additionalProperties":false}),
            output_schema: json!({"type":"object","properties":{
                "count":{"type":"integer","minimum":0},
                "tickets":{"type":"array","items":{"type":"object","properties":{"id":{"type":"integer"},"open":{"type":"boolean"}},"required":["id","open"],"additionalProperties":false}}
            },"required":["count","tickets"],"additionalProperties":false}),
        },
        policy: read_policy(),
    }
}

#[cfg(test)]
mod tests;
