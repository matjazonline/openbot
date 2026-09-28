use super::*;

impl<
    A: WorkflowAuthorization,
    P: WorkflowDefinitions,
    D: compiler::SourceDecoder,
    C: PublicationDirectory,
> DefinitionService<A, P, D, C>
{
    pub async fn save(&self, request: SaveDraft) -> LifecycleResult<CompanyWorkflowDraft> {
        self.authorize(request.target).await?;
        request.content.check()?;
        let current = self
            .persistence
            .draft(request.target.company, request.target.workflow)
            .await?;
        let origin = match (&current, request.expected) {
            (None, None) => None,
            (Some(state), Some(expected)) => {
                check_draft(state, request.target, expected)?;
                state.draft.origin().cloned()
            }
            _ => return Err(conflict().into()),
        };
        let revision =
            DraftRevision::new(next(request.expected.map_or(0, DraftRevision::get))?).unwrap();
        let draft = CompanyWorkflowDraft::saved(
            request.target.company,
            request.target.workflow,
            revision,
            request.content,
            origin,
        );
        self.persistence
            .save_draft(PreparedDraft {
                actor: request.target.actor,
                expected: request.expected,
                state: DraftState {
                    draft: draft.clone(),
                    archived: false,
                },
            })
            .await?;
        Ok(draft)
    }

    pub async fn validate(
        &self,
        target: DefinitionTarget,
        expected: DraftRevision,
    ) -> LifecycleResult<Arc<PublishedBundle>> {
        self.authorize(target).await?;
        let state = self
            .persistence
            .draft(target.company, target.workflow)
            .await?
            .ok_or_else(missing)?;
        check_draft(&state, target, expected)?;
        // This identity is a preview only and is never persisted or selectable.
        self.compile(
            &state.draft,
            target.actor,
            VersionId::new(uuid::Uuid::nil()),
        )
        .await
    }

    pub async fn publish(&self, request: PublishDraft) -> LifecycleResult<Arc<PublishedBundle>> {
        self.authorize(request.target).await?;
        if request.version.as_uuid().is_nil() {
            return Err(
                AppError::BadRequest("A publication version identity is required".into()).into(),
            );
        }
        if let Some(existing) = self
            .persistence
            .publication(request.target.company, &request.key)
            .await?
        {
            if !request.equivalent(&existing.request) {
                return Err(conflict().into());
            }
            check_bundle(
                &existing.bundle,
                request.target.company,
                request.target.workflow,
                request.version,
            )?;
            let result = self
                .persistence
                .publish(PreparedPublication {
                    command: PublishedCommand {
                        request: request.clone(),
                        draft: existing.draft,
                        bundle: existing.bundle,
                    },
                })
                .await?;
            check_bundle(
                &result,
                request.target.company,
                request.target.workflow,
                request.version,
            )?;
            return Ok(result);
        }
        let state = self
            .persistence
            .draft(request.target.company, request.target.workflow)
            .await?
            .ok_or_else(missing)?;
        check_draft(&state, request.target, request.expected)?;
        let bundle = self
            .compile(&state.draft, request.target.actor, request.version)
            .await?;
        let result = self
            .persistence
            .publish(PreparedPublication {
                command: PublishedCommand {
                    request: request.clone(),
                    draft: state.draft,
                    bundle,
                },
            })
            .await?;
        check_bundle(
            &result,
            request.target.company,
            request.target.workflow,
            request.version,
        )?;
        Ok(result)
    }

    pub async fn archive(
        &self,
        target: DefinitionTarget,
        expected: DraftRevision,
    ) -> LifecycleResult<()> {
        self.authorize(target).await?;
        let state = self
            .persistence
            .draft(target.company, target.workflow)
            .await?
            .ok_or_else(missing)?;
        check_draft(&state, target, expected)?;
        let draft = &state.draft;
        let archived = CompanyWorkflowDraft::saved(
            target.company,
            target.workflow,
            DraftRevision::new(next(expected.get())?).unwrap(),
            DraftContent {
                title: draft.title().into(),
                description: draft.description().into(),
                source: draft.source().into(),
            },
            draft.origin().cloned(),
        );
        self.persistence
            .save_draft(PreparedDraft {
                actor: target.actor,
                expected: Some(expected),
                state: DraftState {
                    draft: archived,
                    archived: true,
                },
            })
            .await?;
        Ok(())
    }

    async fn authorize(&self, target: DefinitionTarget) -> AppResult<()> {
        if target.company.as_uuid().is_nil() || target.workflow.as_uuid().is_nil() {
            return Err(AppError::BadRequest(
                "Non-nil company and workflow identities required".into(),
            ));
        }
        self.authorization
            .authorize(
                target.company,
                target.actor,
                RelatedAssociation::Company,
                WorkflowOperation::ManageDefinition,
            )
            .await
    }

    async fn compile(
        &self,
        draft: &CompanyWorkflowDraft,
        actor: WorkflowActor,
        version: VersionId,
    ) -> LifecycleResult<Arc<PublishedBundle>> {
        let decoded = self.decoder.decode(draft.source())?;
        let identity_span = workflow_identity_span(&decoded);
        let captured = self.directory.capture(actor, draft).await?;
        let bundle = publication::freeze(
            decoded,
            draft.company(),
            version,
            captured.snapshots,
            captured.children,
        )?;
        if bundle.compiled().graph().definition().workflow_id != draft.workflow() {
            return Err(compiler::Diagnostic::at(
                "workflow.identity",
                "workflow_id must match the workflow draft being edited",
                "/workflow_id",
                identity_span,
            )
            .into());
        }
        Ok(Arc::new(bundle))
    }
}

fn workflow_identity_span(decoded: &compiler::DecodedSource) -> compiler::SourceSpan {
    if let compiler::NodeValue::Mapping(fields) = &decoded.root.value {
        for (key, value) in fields {
            if matches!(&key.value, compiler::NodeValue::Scalar(key) if key.as_str() == Some("workflow_id"))
            {
                return value.span;
            }
        }
    }
    decoded.root.span
}

fn check_draft(
    state: &DraftState,
    target: DefinitionTarget,
    revision: DraftRevision,
) -> AppResult<()> {
    if state.draft.company() != target.company || state.draft.workflow() != target.workflow {
        return Err(missing());
    }
    if state.archived || state.draft.revision() != revision {
        return Err(conflict());
    }
    Ok(())
}

pub(super) fn check_bundle(
    bundle: &PublishedBundle,
    company: CompanyId,
    workflow: WorkflowId,
    version: VersionId,
) -> AppResult<()> {
    let definition = bundle.compiled().graph().definition();
    if bundle.company_id() != company
        || definition.workflow_id != workflow
        || definition.version_id != version
    {
        return Err(missing());
    }
    Ok(())
}
