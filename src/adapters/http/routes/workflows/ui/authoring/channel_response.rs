use super::super::super::channel_replies::{ResponderBody, ResponseBody, save};
use super::*;
use crate::application::workflow::RelatedChannelId;
use crate::application::workflow::channel_reply::ChannelReplyContract;

fn typed(fields: &FormFields) -> pages::ChannelResponseForm {
    pages::ChannelResponseForm {
        revision: fields.text("expected_revision"),
        selection: fields.text("responder"),
        contract: fields.text("contract"),
        approval_policy: fields.text("approval_policy"),
    }
}

fn command(form: &pages::ChannelResponseForm) -> AppResult<ResponseBody> {
    let invalid = || {
        AppError::BadRequest(
            "Choose a current binding and valid response mapping or approval policy".into(),
        )
    };
    let expected_revision = if form.revision.is_empty() {
        None
    } else {
        Some(form.revision.parse().map_err(|_| invalid())?)
    };
    let responder = if form.selection.is_empty() {
        None
    } else {
        let (id, revision) = form.selection.split_once(':').ok_or_else(invalid)?;
        Some(ResponderBody {
            binding_id: Uuid::parse_str(id).map_err(|_| invalid())?,
            expected_binding_state_revision: revision.parse().map_err(|_| invalid())?,
            contract: serde_json::from_str::<ChannelReplyContract>(&form.contract)
                .map_err(|_| invalid())?,
        })
    };
    let approval_policy = if responder.is_none() || form.approval_policy.trim().is_empty() {
        None
    } else {
        Some(serde_json::from_str(&form.approval_policy).map_err(|_| invalid())?)
    };
    Ok(ResponseBody {
        expected_revision,
        responder,
        approval_policy,
    })
}

pub(super) async fn card(
    ui: &Ui,
    company: Uuid,
    channel: Uuid,
    draft: Option<&pages::ChannelResponseForm>,
    error: Option<&str>,
) -> AppResult<String> {
    let settings = ui
        .engine
        .channel_replies()
        .channel_reply_settings(
            super::company(company),
            RelatedChannelId::new(channel),
            ui.actor()?,
        )
        .await?;
    let usage = ui
        .engine
        .authoring_reads()
        .channel_usage(super::company(company), channel)
        .await?;
    let response = pages::ChannelResponseSettingsView {
        company,
        channel,
        settings: settings.as_ref(),
        bindings: &usage.bindings,
        draft,
        error,
    };
    Ok(pages::channel_bindings_card(&pages::ChannelBindingsCard {
        company_id: company,
        bindings: &usage.bindings,
        truncated: usage.truncated,
        response: Some(&response),
    }))
}

pub(super) async fn save_response(
    ui: Ui,
    Path(channel): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
    body: Bytes,
) -> WorkflowResult<Html<String>> {
    let company = ui.company(query.company_id).await?.id;
    // Scope the channel before returning any validation text or binding choices.
    ui.engine
        .channel_replies()
        .channel_reply_settings(
            super::company(company),
            RelatedChannelId::new(channel),
            ui.actor()?,
        )
        .await?;
    let form = typed(&FormFields::parse(&body));
    let result = match command(&form) {
        Ok(request) => save(&ui.engine, company, channel, ui.actor()?, request).await,
        Err(error) => Err(error),
    };
    match result {
        Ok(_) => Ok(Html(card(&ui, company, channel, None, None).await?)),
        Err(error @ (AppError::Conflict(_) | AppError::BadRequest(_))) => Ok(Html(
            card(&ui, company, channel, Some(&form), Some(&error.to_string())).await?,
        )),
        Err(error) => Err(error.into()),
    }
}
