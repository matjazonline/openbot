use super::*;

pub(super) struct SourceBinding {
    pub(super) source: Value,
    pub(super) params: Value,
    pub(super) trigger: Option<MessageTrigger>,
}

impl Deployment {
    pub(super) async fn bind_source(&self, request: SourceBinding) -> BindingTarget {
        let SourceBinding {
            mut source,
            params,
            trigger,
        } = request;
        let workflow = WorkflowId::new(Uuid::new_v4());
        source["workflow_id"] = json!(workflow.to_string());
        let definitions = self
            .engine
            .services()
            .definitions(Vec::new(), self.mcp_selections.clone());
        let definition = DefinitionTarget {
            workflow,
            ..self.fixture.target
        };
        let mut draft = self.fixture.save(None);
        draft.target = definition;
        draft.content.source = source.to_string();
        definitions.save(draft).await.unwrap();
        let mut publication = self.fixture.publish();
        publication.target = definition;
        publication.key =
            IdempotencyKey::parse(format!("publication-{}", workflow.as_uuid().simple())).unwrap();
        let version = publication.version;
        definitions.publish(publication).await.unwrap();
        let target = BindingTarget {
            company: self.fixture.target.company,
            actor: self.fixture.target.actor,
            binding: WorkflowBindingId::new(Uuid::new_v4()),
        };
        let state = self
            .engine
            .services()
            .bindings()
            .configure(ConfigureBinding {
                target,
                expected: None,
                association: RelatedAssociation::Channel(RelatedChannelId::new(self.channel)),
                version,
                params,
                resources: self.resources.clone(),
                run_as: None,
                message_trigger: trigger,
            })
            .await
            .unwrap();
        self.engine
            .services()
            .bindings()
            .activate(SetBindingActivity {
                target,
                expected: state.revision,
            })
            .await
            .unwrap();
        target
    }
}
