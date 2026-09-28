use super::*;

impl<A: WorkflowAuthorization, P: BindingLifecycle, V: WorkflowDefinitions, R: ResourceDirectory>
    BindingService<A, P, V, R>
{
    pub async fn configure(&self, request: ConfigureBinding) -> LifecycleResult<BindingState> {
        self.authorize(request.target, request.association).await?;
        let current = self
            .persistence
            .binding(request.target.company, request.target.binding)
            .await?;
        let (configuration_revision, active) = match (&current, request.expected) {
            (None, None) => (1, false),
            (Some(state), Some(expected)) => {
                check_state(state, request.target, expected)?;
                if state.association != request.association {
                    return Err(conflict().into());
                }
                (next(state.configuration.revision().get())?, state.active)
            }
            _ => return Err(conflict().into()),
        };
        let bundle = self
            .versions
            .selectable_version(request.target.company, request.version)
            .await?
            .ok_or_else(missing)?;
        if bundle.company_id() != request.target.company
            || bundle.compiled().graph().definition().version_id != request.version
        {
            return Err(missing().into());
        }
        let configuration = Arc::new(ConfiguredBinding::new(
            BindingConfiguration {
                id: request.target.binding,
                revision: BindingRevision::new(configuration_revision).unwrap(),
                company_id: request.target.company,
                params: request.params,
                resources: request.resources,
            },
            bundle,
        )?);
        // Configuration cannot retain foreign/revoked resources even while inactive.
        configuration
            .check_readiness(request.target.actor, &self.resources)
            .await?;
        let state = BindingState {
            configuration,
            association: request.association,
            revision: BindingStateRevision::new(next(
                request.expected.map_or(0, BindingStateRevision::get),
            )?)
            .unwrap(),
            active,
        };
        self.persistence
            .save_binding(PreparedBinding {
                actor: request.target.actor,
                expected: request.expected,
                state: state.clone(),
            })
            .await?;
        Ok(state)
    }

    pub async fn activate(&self, request: SetBindingActivity) -> LifecycleResult<BindingState> {
        let mut state = self.load(request).await?;
        let bundle = state.configuration.bundle();
        let version = bundle.compiled().graph().definition().version_id;
        let selectable = self
            .versions
            .selectable_version(request.target.company, version)
            .await?
            .ok_or_else(missing)?;
        definitions::check_bundle(
            &selectable,
            request.target.company,
            bundle.compiled().graph().definition().workflow_id,
            version,
        )?;
        if selectable.content_hash() != bundle.content_hash() {
            return Err(conflict().into());
        }
        state
            .configuration
            .check_readiness(request.target.actor, &self.resources)
            .await?;
        state.active = true;
        self.persist_activity(request, state).await
    }

    pub async fn deactivate(&self, request: SetBindingActivity) -> LifecycleResult<BindingState> {
        let mut state = self.load(request).await?;
        state.active = false;
        self.persist_activity(request, state).await
    }

    async fn load(&self, request: SetBindingActivity) -> LifecycleResult<BindingState> {
        self.authorize(request.target, RelatedAssociation::Company)
            .await?;
        let state = self
            .persistence
            .binding(request.target.company, request.target.binding)
            .await?
            .ok_or_else(missing)?;
        check_state(&state, request.target, request.expected)?;
        self.authorize(request.target, state.association).await?;
        Ok(state)
    }

    async fn persist_activity(
        &self,
        request: SetBindingActivity,
        mut state: BindingState,
    ) -> LifecycleResult<BindingState> {
        state.revision = BindingStateRevision::new(next(request.expected.get())?).unwrap();
        self.persistence
            .save_binding(PreparedBinding {
                actor: request.target.actor,
                expected: Some(request.expected),
                state: state.clone(),
            })
            .await?;
        Ok(state)
    }

    async fn authorize(
        &self,
        target: BindingTarget,
        association: RelatedAssociation,
    ) -> AppResult<()> {
        if target.company.as_uuid().is_nil() || target.binding.as_uuid().is_nil() {
            return Err(AppError::BadRequest(
                "Non-nil company and binding identities required".into(),
            ));
        }
        self.authorization
            .authorize(
                target.company,
                target.actor,
                association,
                WorkflowOperation::ManageBinding,
            )
            .await
    }
}

fn check_state(
    state: &BindingState,
    target: BindingTarget,
    expected: BindingStateRevision,
) -> AppResult<()> {
    if state.configuration.company_id() != target.company
        || state.configuration.id() != target.binding
    {
        return Err(missing());
    }
    if state.revision != expected {
        return Err(conflict());
    }
    Ok(())
}
