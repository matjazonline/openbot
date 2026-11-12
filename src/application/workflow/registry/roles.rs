use super::*;
use crate::entities::department::DepartmentRole;

impl Registration<'_> {
    pub(super) fn agent_or_role(&mut self) -> Result<Option<&'static str>, Diagnostic> {
        if ModelStep::of(self.name()).is_none() {
            return Ok(None);
        }
        let inputs = &self.parsed.definition.steps[self.id].inputs;
        match (inputs.contains_key("agent"), inputs.contains_key("role")) {
            (true, false) => Ok(Some("agent")),
            (false, true) => {
                let value = self.literal("role").map_err(|_| {
                    self.error(
                        "registry.role",
                        "Choose a literal role: lead or reviewer",
                        "role",
                    )
                })?;
                if value
                    .as_str()
                    .and_then(|value| value.parse::<DepartmentRole>().ok())
                    .is_none()
                {
                    return Err(self.error(
                        "registry.role",
                        "Unknown role; choose lead or reviewer",
                        "role",
                    ));
                }
                Ok(Some("role"))
            }
            _ => Err(self.error(
                "registry.agent_or_role",
                "Supply exactly one of agent or role",
                "agent",
            )),
        }
    }
}

pub(super) fn selector_contract(contract: &mut Contract, selector: &str) {
    contract.input["properties"]["role"] =
        json!({"enum": DepartmentRole::ALL.map(DepartmentRole::as_str)});
    if selector == "role" {
        let required = contract.input["required"]
            .as_array_mut()
            .expect("object contract");
        required.retain(|field| field != "agent");
        required.push(json!("role"));
    }
    contract.input["oneOf"] = json!([
        {"required":["agent"],"not":{"required":["role"]}},
        {"required":["role"],"not":{"required":["agent"]}}
    ]);
}
