//! Lifecycle commands for an assistant provisioned in one transaction (p08 D2.3). They are built
//! through the checks the definition and binding services run, for a caller that already holds
//! the company lock and writes them on its own transaction.
use super::*;

/// The publication of a provisioned draft. As `DefinitionService::publish` holds it, the bundle
/// must be the draft's own workflow at `request.version`.
pub fn provisioned_publication(
    request: PublishDraft,
    draft: CompanyWorkflowDraft,
    bundle: Arc<PublishedBundle>,
) -> LifecycleResult<PreparedPublication> {
    if draft.company() != request.target.company || draft.workflow() != request.target.workflow {
        return Err(missing().into());
    }
    definitions::check_bundle(
        &bundle,
        request.target.company,
        request.target.workflow,
        request.version,
    )?;
    Ok(PreparedPublication {
        command: PublishedCommand {
            request,
            draft,
            bundle,
        },
    })
}

/// A new message-triggered binding running as its configuring actor, configured and then
/// activated: the two changes `BindingService::configure` and `activate` would prepare.
///
/// - Configure: `ConfiguredBinding::new` (company, parameters, slots) and
///   `check_message_trigger`. Readiness has nothing to resolve here, because a provisioned
///   template declares no resources; persistence re-checks the tools under its lock.
/// - Activate: the bundle checks hold by construction (the version is the one just published
///   in the same transaction), and persistence re-reads it in `verify_selection`.
pub fn provisioned_binding(
    target: BindingTarget,
    association: RelatedAssociation,
    params: serde_json::Value,
    message_trigger: crate::domain::workflow::MessageTrigger,
    bundle: Arc<PublishedBundle>,
) -> LifecycleResult<[PreparedBinding; 2]> {
    if !bundle.compiled().graph().definition().resources.is_empty() {
        return Err(AppError::BadRequest(
            "A provisioned workflow cannot declare resource slots".into(),
        )
        .into());
    }
    let configuration = Arc::new(ConfiguredBinding::new(
        BindingConfiguration {
            id: target.binding,
            revision: BindingRevision::new(1).expect("first configuration"),
            company_id: target.company,
            params,
            resources: Default::default(),
            run_as: target.actor,
            message_trigger: Some(message_trigger),
        },
        bundle,
    )?);
    configuration.check_message_trigger(association)?;
    let configured = BindingState {
        configuration,
        association,
        revision: BindingStateRevision::new(1).expect("first state"),
        active: false,
    };
    let activated = BindingState {
        revision: BindingStateRevision::new(2).expect("second state"),
        active: true,
        ..configured.clone()
    };
    Ok([
        PreparedBinding {
            actor: target.actor,
            expected: None,
            state: configured,
        },
        PreparedBinding {
            actor: target.actor,
            expected: Some(BindingStateRevision::new(1).expect("first state")),
            state: activated,
        },
    ])
}

/// `current` reconfigured onto `bundle` -- its own bundle, or a re-pointed version -- to run as
/// `actor`, with its parameters, resources, trigger and active flag unchanged: the change
/// `BindingService::configure` would prepare, for a department desk following its lead
/// (01DEP-P03-D9). The caller holds the company lock and writes it on its own transaction.
pub fn provisioned_reconfiguration(
    actor: WorkflowActor,
    current: &BindingState,
    bundle: Arc<PublishedBundle>,
) -> LifecycleResult<PreparedBinding> {
    let old = &current.configuration;
    let configuration = Arc::new(ConfiguredBinding::new(
        BindingConfiguration {
            id: old.id(),
            revision: old
                .revision()
                .get()
                .checked_add(1)
                .and_then(BindingRevision::new)
                .ok_or_else(missing)?,
            company_id: old.company_id(),
            params: old.params().clone(),
            resources: old.resources().clone(),
            run_as: actor,
            message_trigger: old.message_trigger(),
        },
        bundle,
    )?);
    configuration.check_message_trigger(current.association)?;
    Ok(PreparedBinding {
        actor,
        expected: Some(current.revision),
        state: BindingState {
            configuration,
            association: current.association,
            revision: next_state(current.revision)?,
            active: current.active,
        },
    })
}

/// `current` activated or paused as it is configured: the change `BindingService::activate` or
/// `deactivate` would prepare.
pub fn provisioned_activation(
    actor: WorkflowActor,
    current: &BindingState,
    active: bool,
) -> LifecycleResult<PreparedBinding> {
    Ok(PreparedBinding {
        actor,
        expected: Some(current.revision),
        state: BindingState {
            revision: next_state(current.revision)?,
            active,
            ..current.clone()
        },
    })
}

fn next_state(revision: BindingStateRevision) -> LifecycleResult<BindingStateRevision> {
    Ok(revision
        .get()
        .checked_add(1)
        .and_then(BindingStateRevision::new)
        .ok_or_else(missing)?)
}
