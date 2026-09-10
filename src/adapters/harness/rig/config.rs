//! Compile only the reviewed Rig settings. Provider attempts share one application run budget;
//! a library's continuation limit is not an equivalent accounting primitive.
use crate::{
    app_error::{AppError, AppResult},
    entities::harness::{AgentCapabilitySpec, HarnessKind},
};

pub struct ModelCallBudget {
    remaining: u8,
}
impl ModelCallBudget {
    pub fn compile(spec: &AgentCapabilitySpec, server_ceiling: u8) -> AppResult<Self> {
        if spec.harness != HarnessKind::Rig {
            return Err(AppError::BadRequest(
                "Rig compiler requires harness_kind=rig".into(),
            ));
        }
        let config = spec.harness_config.rig().ok_or_else(|| {
            AppError::BadRequest("Rig compiler requires Rig advanced config".into())
        })?;
        config.validate().map_err(AppError::BadRequest)?;
        Ok(Self {
            remaining: config.effective_max_turns(server_ceiling),
        })
    }
    /// Charge immediately before each model call, including failed calls, recovery, and skills.
    pub fn charge(&mut self) -> AppResult<()> {
        self.remaining = self
            .remaining
            .checked_sub(1)
            .ok_or_else(|| AppError::BadRequest("Rig model-call budget exhausted".into()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::harness::{HarnessConfig, SubAgentScope};
    #[test]
    fn every_attempt_consumes_the_same_budget_and_mismatches_fail() {
        let mut spec = AgentCapabilitySpec {
            response_contract: None,
            harness: HarnessKind::Rig,
            name: "test".into(),
            system_prompt: "test".into(),
            provider: "openai".into(),
            model: "model".into(),
            provider_base_url: None,
            skills: vec![],
            granted_tools: vec![],
            sub_agents: SubAgentScope::AllCompanySiblings,
            harness_config: HarnessConfig::empty(HarnessKind::Rig),
        };
        let mut budget = ModelCallBudget::compile(&spec, 3).unwrap();
        for _ in 0..3 {
            budget.charge().unwrap();
        }
        assert!(budget.charge().is_err());
        assert!(budget.charge().is_err());
        assert!(
            ModelCallBudget::compile(&spec, 0)
                .unwrap()
                .charge()
                .is_err()
        );
        spec.harness_config = HarnessConfig::empty(HarnessKind::AiAgents);
        assert!(ModelCallBudget::compile(&spec, 3).is_err());
        spec.harness = HarnessKind::AiAgents;
        assert!(ModelCallBudget::compile(&spec, 3).is_err());
    }
}
