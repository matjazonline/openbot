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
    copy_model: LibraryCopyModel,
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
                message: format!("'{}' was created on the company's default model: the model it is published with is not enabled here.", definition.name),
            });
        }
        Err(error) => return Err(error),
        Ok(()) => {}
    }
    if copy_model == LibraryCopyModel::PinDefault
        && write.provider.is_none()
        && let Some((provider, model)) = pinned_default_model(connections)
    {
        write.provider = Some(provider);
        write.model = Some(model);
    }
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
    let default = connections
        .iter()
        .find(|connection| connection.is_default)
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
