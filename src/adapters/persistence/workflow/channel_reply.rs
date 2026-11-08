//! Selection and frozen admission ownership. Publication is a later durable owner.
use super::*;
use crate::application::workflow::channel_reply::{
    ChannelReplies, ChannelReplyContract, ChannelReplySettings, ConfigureChannelReply,
    ConfigureResponder, ReplySettingsRevision, ReplyText, SelectedResponder,
};
use serde_json::Value;

#[derive(sqlx::FromRow)]
struct SettingsRow {
    revision: i64,
    binding_id: Option<Uuid>,
    binding_revision: Option<i64>,
    contract: Option<Value>,
    approval_policy: Option<Value>,
}

impl SettingsRow {
    fn settings(self) -> AppResult<ChannelReplySettings> {
        let responder = match (self.binding_id, self.binding_revision, self.contract) {
            (None, None, None) => None,
            (Some(binding), Some(revision), Some(contract)) => Some(SelectedResponder {
                binding: WorkflowBindingId::new(binding),
                configuration_revision: BindingRevision::new(
                    u64::try_from(revision).map_err(|_| invalid())?,
                )
                .ok_or_else(invalid)?,
                contract: serde_json::from_value(contract).map_err(|_| invalid())?,
            }),
            _ => return Err(invalid()),
        };
        Ok(ChannelReplySettings {
            revision: ReplySettingsRevision::try_from(
                u64::try_from(self.revision).map_err(|_| invalid())?,
            )
            .map_err(|_| invalid())?,
            responder,
            approval_policy: self
                .approval_policy
                .map(serde_json::from_value)
                .transpose()
                .map_err(|_| invalid())?,
        })
    }
}

async fn settings_on(
    db: &mut PgConnection,
    company: CompanyId,
    channel: RelatedChannelId,
) -> AppResult<Option<ChannelReplySettings>> {
    sqlx::query_as::<_, SettingsRow>(
        "SELECT revision,binding_id,binding_revision,contract,approval_policy FROM channel_reply_settings \
         WHERE company_id=$1 AND channel_id=$2",
    )
    .bind(company.as_uuid())
    .bind(channel.as_uuid())
    .fetch_optional(db)
    .await?
    .map(SettingsRow::settings)
    .transpose()
}

/// Only the just-created native binding is selected, inside provisioning's savepoint.
/// Caller authorization and account locks already belong to the provisioning transaction.
pub(super) async fn bootstrap_native_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    channel: RelatedChannelId,
    actor: WorkflowActor,
    binding: WorkflowBindingId,
) -> AppResult<()> {
    authorize_settings(tx, company, channel, actor).await?;
    if settings_on(tx, company, channel).await?.is_some() {
        return Err(conflict());
    }
    let binding_state = binding_rows::read_binding(tx, company, binding)
        .await?
        .ok_or_else(missing)?;
    if binding_state.configuration.run_as() != actor {
        return Err(conflict());
    }
    let request = ConfigureChannelReply {
        company,
        channel,
        actor,
        expected: None,
        responder: Some(ConfigureResponder {
            binding,
            expected_binding_revision: binding_state.revision,
            contract: ChannelReplyContract {
                format_version: 1,
                subject: ReplyText::Literal {
                    literal: "Response".into(),
                },
                body: ReplyText::Reference {
                    r#ref: "/input/body"
                        .to_owned()
                        .try_into()
                        .expect("native response reference"),
                },
            },
        }),
        approval_policy: None,
    };
    let state = ChannelReplySettings {
        revision: ReplySettingsRevision::try_from(1).expect("initial revision"),
        responder: selected_on(tx, &request).await?,
        approval_policy: None,
    };
    write_settings(tx, &request, &state).await
}

#[async_trait]
impl ChannelReplies for PostgresPersistence {
    async fn channel_reply_settings(
        &self,
        company: CompanyId,
        channel: RelatedChannelId,
        actor: WorkflowActor,
    ) -> AppResult<Option<ChannelReplySettings>> {
        let mut tx = self.pool.begin().await?;
        authorize_settings(&mut tx, company, channel, actor).await?;
        let settings = settings_on(&mut tx, company, channel).await?;
        tx.commit().await?;
        Ok(settings)
    }

    async fn configure_channel_reply(
        &self,
        request: ConfigureChannelReply,
    ) -> AppResult<ChannelReplySettings> {
        let mut tx = self.pool.begin().await?;
        lock_settings_accounts(&mut tx, &request).await?;
        authorize_settings(&mut tx, request.company, request.channel, request.actor).await?;
        sqlx::query("SELECT id FROM channels WHERE company_id=$1 AND id=$2 FOR UPDATE")
            .bind(request.company.as_uuid())
            .bind(request.channel.as_uuid())
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(missing)?;
        let current = settings_on(&mut tx, request.company, request.channel).await?;
        if current.as_ref().map(|value| value.revision) != request.expected {
            return Err(conflict());
        }
        let responder = selected_on(&mut tx, &request).await?;
        validate_reviewers(&mut tx, &request).await?;
        let next = request
            .expected
            .map_or(0, ReplySettingsRevision::get)
            .checked_add(1)
            .ok_or_else(conflict)?;
        let state = ChannelReplySettings {
            revision: ReplySettingsRevision::try_from(next).map_err(|_| conflict())?,
            responder,
            approval_policy: request.approval_policy.clone(),
        };
        write_settings(&mut tx, &request, &state).await?;
        tx.commit().await?;
        tracing::info!(company=%request.company.as_uuid(),channel=%request.channel.as_uuid(),
            revision=next,"Channel responder configured");
        Ok(state)
    }
}

async fn lock_settings_accounts(
    db: &mut PgConnection,
    request: &ConfigureChannelReply,
) -> AppResult<()> {
    let mut accounts = vec![request.actor.user_id()];
    if let Some(policy) = &request.approval_policy {
        accounts.extend_from_slice(policy.reviewers());
    }
    if let Some(responder) = &request.responder {
        let run_as: Option<Uuid> = sqlx::query_scalar("SELECT revision.run_as_user_id FROM workflow_bindings AS binding \
            JOIN workflow_binding_revisions AS revision ON revision.company_id=binding.company_id AND revision.binding_id=binding.id \
            AND revision.revision=binding.configuration_revision WHERE binding.company_id=$1 AND binding.id=$2")
            .bind(request.company.as_uuid()).bind(responder.binding.as_uuid()).fetch_optional(&mut *db).await?;
        accounts.extend(run_as);
    }
    accounts.sort_unstable();
    accounts.dedup();
    sqlx::query("SELECT id FROM users WHERE id=ANY($1) ORDER BY id FOR SHARE")
        .bind(accounts)
        .execute(db)
        .await?;
    Ok(())
}

async fn authorize_settings(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    channel: RelatedChannelId,
    actor: WorkflowActor,
) -> AppResult<()> {
    sqlx::query("SET LOCAL lock_timeout = '5s'")
        .execute(&mut **tx)
        .await?;
    sqlx::query("SET LOCAL statement_timeout = '20s'")
        .execute(&mut **tx)
        .await?;
    authority::authorize_company(tx, company, actor).await?;
    association::authorize_association(tx, company, actor, RelatedAssociation::Channel(channel))
        .await
}

async fn validate_reviewers(
    db: &mut Transaction<'_, Postgres>,
    request: &ConfigureChannelReply,
) -> AppResult<()> {
    let Some(policy) = &request.approval_policy else {
        return Ok(());
    };
    for user in policy.reviewers() {
        let actor = WorkflowActor::authenticated(*user)?;
        let reviewer = authority::authorize_reviewer_on(db, request.company, actor).await?;
        if !reviewer.privileged {
            return Err(AppError::BadRequest(
                "Final reply reviewers must be company admins".into(),
            ));
        }
        association::authorize_association(
            db,
            request.company,
            actor,
            RelatedAssociation::Channel(request.channel),
        )
        .await?;
    }
    Ok(())
}

async fn selected_on(
    tx: &mut Transaction<'_, Postgres>,
    request: &ConfigureChannelReply,
) -> AppResult<Option<SelectedResponder>> {
    let Some(selection) = &request.responder else {
        return Ok(None);
    };
    let state = binding_rows::read_binding(tx, request.company, selection.binding)
        .await?
        .ok_or_else(missing)?;
    if !state.active
        || state.revision != selection.expected_binding_revision
        || state.association != RelatedAssociation::Channel(request.channel)
        || state.configuration.message_trigger().is_none()
    {
        return Err(conflict());
    }
    let configuration = &state.configuration;
    authority::authorize_company(tx, request.company, configuration.run_as()).await?;
    association::authorize_association(
        tx,
        request.company,
        configuration.run_as(),
        RelatedAssociation::Channel(request.channel),
    )
    .await?;
    bindings::verify_selection(tx, configuration).await?;
    resources::check_readiness(tx, configuration.as_ref().into()).await?;
    selection.contract.validate(
        configuration
            .bundle()
            .compiled()
            .graph()
            .definition()
            .output_schema
            .as_ref()
            .unwrap_or(&Value::Bool(true)),
    )?;
    Ok(Some(SelectedResponder {
        binding: selection.binding,
        configuration_revision: configuration.revision(),
        contract: selection.contract.clone(),
    }))
}

async fn write_settings(
    tx: &mut Transaction<'_, Postgres>,
    request: &ConfigureChannelReply,
    state: &ChannelReplySettings,
) -> AppResult<()> {
    let selection = state.responder.as_ref();
    let contract = selection
        .map(|value| serde_json::to_value(&value.contract))
        .transpose()
        .map_err(|_| invalid())?;
    sqlx::query(
        "INSERT INTO channel_reply_settings(company_id,channel_id,revision,binding_id,binding_revision,contract,approval_policy) \
         VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(company_id,channel_id) DO UPDATE \
         SET revision=EXCLUDED.revision,binding_id=EXCLUDED.binding_id,\
         binding_revision=EXCLUDED.binding_revision,contract=EXCLUDED.contract,approval_policy=EXCLUDED.approval_policy")
        .bind(request.company.as_uuid()).bind(request.channel.as_uuid())
        .bind(revision(state.revision.get())?)
        .bind(selection.map(|value| value.binding.as_uuid()))
        .bind(selection.map(|value| revision(value.configuration_revision.get())).transpose()?)
        .bind(contract).bind(state.approval_policy.as_ref().map(serde_json::to_value).transpose().map_err(|_|invalid())?)
        .execute(&mut **tx).await.map_err(write_error)?;
    Ok(())
}

pub(super) async fn frozen_revision(
    db: &mut PgConnection,
    company: CompanyId,
    binding: WorkflowBindingId,
    trigger: &TriggerRef,
) -> AppResult<Option<BindingRevision>> {
    let TriggerSource::Message { message_id } = trigger.source() else {
        return Ok(None);
    };
    sqlx::query_scalar::<_, i64>(
        "SELECT binding_revision FROM channel_reply_contexts WHERE company_id=$1 AND intent_id=$2 \
         AND binding_id=$3 AND message_id=$4",
    )
    .bind(company.as_uuid())
    .bind(trigger.trigger_id().as_uuid())
    .bind(binding.as_uuid())
    .bind(message_id.as_uuid())
    .fetch_optional(db)
    .await?
    .map(|value| {
        BindingRevision::new(u64::try_from(value).map_err(|_| invalid())?).ok_or_else(invalid)
    })
    .transpose()
}

pub(super) async fn link_run(
    tx: &mut Transaction<'_, Postgres>,
    command: &PreparedAdmission,
    run: RunId,
) -> AppResult<()> {
    let Some(AdmissionClaim::Message(claim)) = command.admission_claim() else {
        return Ok(());
    };
    let linked = sqlx::query_scalar::<_,Uuid>(
        "INSERT INTO channel_reply_runs(company_id,intent_id,run_id,binding_id,binding_revision) \
         SELECT company_id,intent_id,$3,binding_id,binding_revision FROM channel_reply_contexts \
         WHERE company_id=$1 AND intent_id=$2 AND binding_id=$4 AND binding_revision=$5 \
         ON CONFLICT(company_id,intent_id) DO UPDATE SET run_id=channel_reply_runs.run_id RETURNING run_id",
    )
    .bind(claim.company.as_uuid())
    .bind(claim.intent)
    .bind(run.as_uuid())
    .bind(command.binding().id().as_uuid())
    .bind(revision(command.binding().revision().get())?)
    .fetch_optional(&mut **tx)
    .await?;
    if linked.is_some_and(|saved| saved != run.as_uuid()) {
        return Err(conflict());
    }
    Ok(())
}
