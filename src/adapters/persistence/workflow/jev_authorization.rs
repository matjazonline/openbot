use super::*;
use crate::application::workflow::{
    actions::ActionScope, lease::ClaimedWorkflow, polling::WorkflowStepKind,
};
use serde_json::Value;
use std::{collections::BTreeMap, time::Duration};

#[derive(sqlx::FromRow)]
struct Hints {
    actor_id: Uuid,
    channel_id: Option<Uuid>,
    thread_id: Option<Uuid>,
    bundle: Vec<u8>,
    resources: Value,
}
struct Selection {
    slot: ResourceName,
    id: Uuid,
    model: JevModel,
    questions: Value,
    assets: Option<Value>,
}
fn selection(hints: &Hints, claim: &ClaimedWorkflow) -> AppResult<Selection> {
    let bundle = publication::restore_bundle(
        &hints.bundle,
        &crate::adapters::workflow_source::WorkflowSourceDecoder,
    )?;
    if bundle.company_id() != claim.fence.scope.company {
        return Err(revoked());
    }
    let definition = bundle.compiled().graph().definition();
    let step = definition
        .steps
        .get(&claim.activation.step)
        .ok_or_else(revoked)?;
    if step.step_type.as_str() != "jev.evaluate" {
        return Err(revoked());
    }
    let literal = |name| match step.inputs.get(name) {
        Some(crate::domain::workflow::Binding::Literal(value)) => Ok(value),
        _ => Err(revoked()),
    };
    let slot = ResourceName::parse(literal("connection")?.as_str().ok_or_else(revoked)?)
        .map_err(|_| revoked())?;
    if !definition
        .resources
        .iter()
        .any(|requirement| requirement.slot == slot && requirement.kind.as_str() == "typesafe")
    {
        return Err(revoked());
    }
    let resources: BTreeMap<ResourceName, RuntimeResourceId> =
        serde_json::from_value(hints.resources.clone()).map_err(|_| revoked())?;
    Ok(Selection {
        id: resources.get(&slot).ok_or_else(revoked)?.as_uuid(),
        slot,
        model: JevModel::parse(literal("model")?.as_str().ok_or_else(revoked)?)?,
        questions: literal("questions")?.clone(),
        assets: step
            .inputs
            .get("assets")
            .map(|binding| match binding {
                crate::domain::workflow::Binding::Literal(value) => Ok(value.clone()),
                _ => Err(revoked()),
            })
            .transpose()?,
    })
}
#[async_trait]
impl JevAuthorization for PostgresPersistence {
    async fn authorize_jev(&self, claim: &ClaimedWorkflow) -> AppResult<JevCredential> {
        // Bound pool acquisition, lock waits and the complete authorization transaction.
        tokio::time::timeout(Duration::from_secs(5), authorize(self, claim))
            .await
            .map_err(|_| AppError::Internal("TypeSafe authorization timed out".into()))?
    }
}
async fn authorize(p: &PostgresPersistence, claim: &ClaimedWorkflow) -> AppResult<JevCredential> {
    let mut tx = http::begin_http(p).await?;
    let result = authorize_on(p, &mut tx, claim).await?;
    tx.commit().await?;
    Ok(result)
}

pub(in crate::adapters::persistence::workflow) async fn authorize_on(
    p: &PostgresPersistence,
    tx: &mut Transaction<'_, Postgres>,
    claim: &ClaimedWorkflow,
) -> AppResult<JevCredential> {
    let started = tokio::time::Instant::now();
    let scope = claim.fence.scope;
    let hints: Hints = sqlx::query_as(
        "SELECT actor_id,channel_id,thread_id,bundle,resources FROM workflow_runs WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(revoked)?;
    let actor = WorkflowActor::authenticated(hints.actor_id)?;
    authority::authorize_company(tx, scope.company, actor).await?;
    association::authorize_association(
        tx,
        scope.company,
        actor,
        binding_rows::association_from_columns(hints.channel_id, hints.thread_id)?,
    )
    .await?;
    let selected = selection(&hints, claim)?;
    let assets = crate::application::workflow::jev::parse_assets(selected.assets.as_ref())
        .map_err(|_| revoked())?;
    // File visibility/membership stays locked through credential and fence waits.
    super::super::jev_assets::locate_on(tx, scope.company, actor, assets).await?;
    // All mutable authority is locked before workflow-local rows. The same connection row
    // protects enabled/model changes, bearer replacement and KEK rewrap.
    let current = connection_on(tx, scope.company, selected.id)
        .await?
        .ok_or_else(revoked)?;
    if !current.enabled || !current.models.contains(&selected.model) {
        return Err(revoked());
    }
    let envelope: String = sqlx::query_scalar(
        "SELECT envelope FROM company_typesafe_connections WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(selected.id)
    .fetch_one(&mut **tx)
    .await?;
    if !lease::lock_fence(tx, claim.fence, Duration::from_secs(2)).await? {
        return Err(AppError::Execution(
            crate::app_error::ExecutionFailure::OwnershipLost,
        ));
    }
    verify_locked(tx, claim, &hints, &selected).await?;
    let bearer = p
        .credential_cipher()?
        .open_envelope(
            &CredentialContext::typesafe_credential(scope.company.as_uuid(), selected.id),
            &envelope,
        )
        .map_err(|_| AppError::InvalidCredentials)?;
    validate_secret(&bearer)?;
    if lease::live_window(tx, claim.fence, started)
        .await?
        .is_none()
    {
        return Err(AppError::Execution(
            crate::app_error::ExecutionFailure::OwnershipLost,
        ));
    }
    Ok(JevCredential {
        connection_id: selected.id,
        model: selected.model,
        bearer,
    })
}

async fn verify_locked(
    tx: &mut Transaction<'_, Postgres>,
    claim: &ClaimedWorkflow,
    hints: &Hints,
    selected: &Selection,
) -> AppResult<()> {
    let scope = claim.fence.scope;
    let locked: Hints = sqlx::query_as(
        "SELECT actor_id,channel_id,thread_id,bundle,resources FROM workflow_runs WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_one(&mut **tx)
    .await?;
    if hints.actor_id != locked.actor_id
        || hints.channel_id != locked.channel_id
        || hints.thread_id != locked.thread_id
        || hints.bundle != locked.bundle
        || hints.resources != locked.resources
    {
        return Err(revoked());
    }
    let facts = http::runtime::facts(
        tx,
        ActionScope {
            company: scope.company,
            run: scope.run,
            execution: scope.execution,
        },
    )
    .await?;
    let inputs =
        http::runtime::frozen_inputs(tx, &facts, claim, &WorkflowStepKind("jev.evaluate".into()))
            .await?;
    if inputs.get("model").and_then(Value::as_str) != Some(selected.model.as_str())
        || inputs.get("questions") != Some(&selected.questions)
        || inputs.get("assets") != selected.assets.as_ref()
        || inputs.get("connection").and_then(Value::as_str) != Some(selected.slot.as_str())
    {
        return Err(revoked());
    }
    crate::application::workflow::registry::jev::validate_inputs(&inputs).map_err(|_| revoked())?;
    Ok(())
}
