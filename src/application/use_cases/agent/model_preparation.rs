//! Pure model decisions shared by normal writes and transaction-owned onboarding.
use super::*;
use crate::entities::company::CompanyModelConnection;

pub(crate) fn validate_model_selection(
    connections: &[CompanyModelConnection],
    write: &AgentWrite,
) -> AppResult<()> {
    let (Some(provider), Some(model)) = (write.provider.as_deref(), write.model.as_deref()) else {
        return if write.provider.is_some() || write.model.is_some() {
            Err(AppError::BadRequest("Select both a provider and one of its enabled models, or inherit the company default.".into()))
        } else {
            Ok(())
        };
    };
    if !model_allowed(connections, provider, model) {
        return Err(AppError::BadRequest(format!(
            "Model '{}' is not enabled for provider '{}' in this company.",
            ModelName::canonical(model),
            ModelProvider::canonical(provider),
        )));
    }
    Ok(())
}

pub(crate) fn prepare_library_write(
    definition: &Agent,
    user_id: Uuid,
    connections: &[CompanyModelConnection],
) -> AppResult<(AgentWrite, Vec<ProvisioningWarning>)> {
    let mut write = library_agent_write(definition);
    let mut warnings = Vec::new();
    match validate_model_selection(connections, &write) {
        Err(AppError::BadRequest(_)) => {
            write.provider = None;
            write.model = None;
            warnings.push(ProvisioningWarning {
                code: "library_model_unavailable".into(),
                message: format!("'{}' was created on its department's default model: the model it is published with is not enabled here or not in its department's reach.", definition.name),
            });
        }
        Err(error) => return Err(error),
        Ok(()) => {}
    }
    // Its desk answers only on an explicit model: a copy that would inherit is pinned inside the
    // creating transaction to its department's default (Plan07 02A.2, 01DEP-P03-D3); with no
    // usable default, creation refuses it.
    write.created_by = Some(CreationProvenance::user(user_id));
    write.normalize()?;
    validate_model_selection(connections, &write)?;
    Ok((write, warnings))
}

pub(crate) fn prompt_model(
    connections: &[CompanyModelConnection],
    provider_override: Option<&str>,
    model_override: Option<&str>,
) -> AppResult<(ModelProvider, ModelName)> {
    // The company default is the root's (Plan07 02A); a department's own default is not it.
    let default = connections
        .iter()
        .find(|connection| connection.is_default && connection.in_root)
        .ok_or_else(|| {
            AppError::BadRequest("Configure a default company model provider first.".into())
        })?;
    let provider =
        ModelProvider::canonical(non_empty(provider_override).unwrap_or(default.provider.as_str()));
    let connection = connections
        .iter()
        .find(|connection| connection.provider == provider)
        .ok_or_else(|| {
            AppError::BadRequest(format!(
                "Provider '{provider}' is not enabled for this company."
            ))
        })?;
    let model = ModelName::canonical(
        non_empty(model_override)
            .or_else(|| connection.models.first().map(|model| model.as_ref()))
            .ok_or_else(|| AppError::BadRequest("Configure at least one model first.".into()))?,
    );
    if !connection.models.contains(&model) {
        return Err(AppError::BadRequest(format!(
            "Model '{model}' is not enabled for provider '{provider}' in this company."
        )));
    }
    Ok((provider, model))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection(provider: &str, in_root: bool, is_default: bool) -> CompanyModelConnection {
        CompanyModelConnection {
            id: Uuid::new_v4(),
            unit_id: Uuid::new_v4(),
            unit_name: if in_root { "Root" } else { "Sales" }.into(),
            in_root,
            provider: provider.into(),
            models: vec![ModelName::from("fixture")],
            is_default,
            has_api_key: true,
        }
    }

    /// Plan07 02A (07UR-P02-B1-F7): only the root's default is the company default; a
    /// department's own default does not stand in for it.
    #[test]
    fn prompt_model_uses_only_the_roots_default() {
        let department_default = [
            connection("openai", true, false),
            connection("anthropic", false, true),
        ];
        assert!(matches!(
            prompt_model(&department_default, None, None),
            Err(AppError::BadRequest(message)) if message.contains("default company model")
        ));
        let rooted = [
            connection("openai", true, true),
            connection("anthropic", false, true),
        ];
        let (provider, model) = prompt_model(&rooted, None, None).unwrap();
        assert_eq!((provider.as_str(), model.as_ref()), ("openai", "fixture"));
    }
}
