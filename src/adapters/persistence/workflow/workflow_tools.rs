//! Workflow tool registrations (07-D2 §D2.1–§D2.2): company-admin commands, the publication
//! snapshot of a granted name, and the publish/run checks of a frozen pin (P07-Q2).
use super::http::begin_http;
use super::*;
use crate::application::workflow::publication::{
    ActionEffect, ActionRecovery, ApprovedActionPolicy, PublishedBundle, WorkflowToolPin,
    restore_bundle,
};
use crate::application::workflow::registry::ToolContract;
use crate::application::workflow::tools::*;
use serde_json::Value;

#[derive(sqlx::FromRow)]
struct RegistrationRow {
    name: String,
    workflow_id: Uuid,
    version_id: Uuid,
    params: Value,
    revision: i64,
}
impl RegistrationRow {
    fn decode(self) -> AppResult<WorkflowToolRegistration> {
        Ok(WorkflowToolRegistration {
            name: TypeName::parse(self.name).map_err(|_| invalid())?,
            workflow: WorkflowId::new(self.workflow_id),
            version: VersionId::new(self.version_id),
            params: self.params,
            revision: self.revision,
        })
    }
}

/// Which rule a frozen pin is held to (P07-Q2, 07-D2 §D2.2).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ToolCheck {
    /// A publication's own freshly captured snapshot: the registration pins exactly it, and the
    /// version is still selectable.
    Publish,
    /// Running a frozen pin (admission, readiness, dispatch, a stored child at publish): the name
    /// is still registered for the same workflow, whatever version it pins now.
    Run,
}

/// The current registration of `name`, read `FOR SHARE` so a revocation or re-point waits.
pub(super) async fn registration_on(
    db: &mut PgConnection,
    company: CompanyId,
    name: &TypeName,
) -> AppResult<Option<WorkflowToolRegistration>> {
    sqlx::query_as::<_, RegistrationRow>(
        "SELECT name, workflow_id, version_id, params, revision FROM workflow_tool_registrations \
         WHERE company_id=$1 AND name=$2 FOR SHARE",
    )
    .bind(company.as_uuid())
    .bind(name.as_str())
    .fetch_optional(&mut *db)
    .await?
    .map(RegistrationRow::decode)
    .transpose()
}

pub(super) async fn registered_on(
    db: &mut PgConnection,
    company: CompanyId,
    tool: &ToolSnapshot,
    check: ToolCheck,
) -> AppResult<()> {
    let ToolKind::Workflow(pin) = tool.kind() else {
        return Err(invalid());
    };
    let registration = registration_on(db, company, &tool.contract.name)
        .await?
        .ok_or_else(missing)?;
    let held = match check {
        ToolCheck::Run => registration.workflow == pin.workflow_id,
        ToolCheck::Publish => {
            registration.workflow == pin.workflow_id
                && registration.version == pin.version_id
                && registration.params == pin.params
                && selectable_bundle(db, company, pin.workflow_id, pin.version_id)
                    .await?
                    .is_some()
        }
    };
    if held { Ok(()) } else { Err(missing()) }
}

/// The stored bundle of a selectable (unarchived) version of this company, `FOR SHARE`.
async fn selectable_bundle(
    db: &mut PgConnection,
    company: CompanyId,
    workflow: WorkflowId,
    version: VersionId,
) -> AppResult<Option<Vec<u8>>> {
    Ok(sqlx::query_scalar(
        "SELECT version.bundle FROM workflow_versions AS version \
         JOIN workflow_definitions AS definition ON definition.company_id = version.company_id \
           AND definition.id = version.workflow_id \
         WHERE version.company_id = $1 AND version.workflow_id = $2 AND version.id = $3 \
           AND NOT definition.archived \
         FOR SHARE OF version, definition",
    )
    .bind(company.as_uuid())
    .bind(workflow.as_uuid())
    .bind(version.as_uuid())
    .fetch_optional(&mut *db)
    .await?)
}

async fn pinned_bundle(
    db: &mut PgConnection,
    company: CompanyId,
    workflow: WorkflowId,
    version: VersionId,
) -> AppResult<Option<Arc<PublishedBundle>>> {
    let Some(bytes) = selectable_bundle(db, company, workflow, version).await? else {
        return Ok(None);
    };
    restore_bundle(
        &bytes,
        &crate::adapters::workflow_source::WorkflowSourceDecoder,
    )
    .map(Some)
}

/// The publication snapshot of the company's workflow tool `name`, if one is registered: the
/// pinned child's own input and output schemas, its static parameters, and a policy that never
/// asks for approval. A pinned version that is no longer selectable cannot be captured.
pub(super) async fn workflow_tool_snapshot(
    db: &mut PgConnection,
    company: CompanyId,
    name: &TypeName,
) -> AppResult<Option<ToolSnapshot>> {
    let Some(registration) = registration_on(db, company, name).await? else {
        return Ok(None);
    };
    let bundle = pinned_bundle(db, company, registration.workflow, registration.version)
        .await?
        .ok_or_else(|| {
            AppError::BadRequest(format!(
                "Workflow tool {} pins a version that is archived or missing",
                name.as_str()
            ))
        })?;
    let definition = bundle.compiled().graph().definition();
    Ok(Some(ToolSnapshot {
        company_id: company,
        contract: ToolContract {
            name: name.clone(),
            connection: None,
            input_schema: definition.input_schema.clone().ok_or_else(invalid)?,
            output_schema: definition.output_schema.clone().ok_or_else(invalid)?,
        },
        policy: ApprovedActionPolicy {
            capability: TypeName::parse("workflow.tool").map_err(|_| invalid())?,
            policy_revision: u64::try_from(registration.revision).map_err(|_| invalid())?,
            effect: ActionEffect::Write,
            recovery: ActionRecovery::SafeRepeat,
        },
        workflow: Some(WorkflowToolPin {
            workflow_id: registration.workflow,
            version_id: registration.version,
            params: registration.params,
        }),
    }))
}

fn refused(message: &str) -> AppError {
    AppError::BadRequest(message.into())
}

#[async_trait]
impl WorkflowTools for PostgresPersistence {
    async fn register_workflow_tool(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        write: WorkflowToolWrite,
    ) -> AppResult<WorkflowToolRegistration> {
        if crate::application::workflow::message::is_builtin_message_tool(write.name.as_str()) {
            return Err(refused("message.* tool names are reserved"));
        }
        if !write.params.is_object()
            || serde_json::to_vec(&write.params)
                .map_err(|_| invalid())?
                .len()
                > MAX_TOOL_PARAMS_BYTES
        {
            return Err(refused(
                "Workflow tool parameters must be an object of at most 64 KiB",
            ));
        }
        let mut tx = begin_http(self).await?;
        // Serializes this company's registration writes (the company row lock) before the count.
        authority::authorize_company(&mut tx, company, actor).await?;
        let bundle = pinned_bundle(&mut tx, company, write.workflow, write.version)
            .await?
            .ok_or_else(missing)?;
        bundle
            .compiled()
            .validate_params(&write.params)
            .map_err(|diagnostic| AppError::BadRequest(diagnostic.message.into()))?;
        let written = match write.expected {
            None => {
                let count: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM workflow_tool_registrations WHERE company_id=$1",
                )
                .bind(company.as_uuid())
                .fetch_one(&mut *tx)
                .await?;
                if count >= MAX_WORKFLOW_TOOLS as i64 {
                    return Err(refused("At most 256 workflow tools are allowed"));
                }
                sqlx::query_as::<_, RegistrationRow>(
                    "INSERT INTO workflow_tool_registrations(company_id,name,workflow_id,version_id,params) \
                     VALUES($1,$2,$3,$4,$5) ON CONFLICT (company_id,name) DO NOTHING \
                     RETURNING name, workflow_id, version_id, params, revision",
                )
                .bind(company.as_uuid())
                .bind(write.name.as_str())
                .bind(write.workflow.as_uuid())
                .bind(write.version.as_uuid())
                .bind(&write.params)
                .fetch_optional(&mut *tx)
                .await?
            }
            Some(revision) => sqlx::query_as::<_, RegistrationRow>(
                "UPDATE workflow_tool_registrations SET workflow_id=$3, version_id=$4, params=$5, \
                 revision=revision+1, updated_at=clock_timestamp() \
                 WHERE company_id=$1 AND name=$2 AND revision=$6 \
                 RETURNING name, workflow_id, version_id, params, revision",
            )
            .bind(company.as_uuid())
            .bind(write.name.as_str())
            .bind(write.workflow.as_uuid())
            .bind(write.version.as_uuid())
            .bind(&write.params)
            .bind(revision)
            .fetch_optional(&mut *tx)
            .await?,
        };
        let registration = written.ok_or_else(conflict)?.decode()?;
        tx.commit().await?;
        Ok(registration)
    }

    async fn revoke_workflow_tool(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        name: TypeName,
        revision: i64,
    ) -> AppResult<()> {
        let mut tx = begin_http(self).await?;
        authority::authorize_company(&mut tx, company, actor).await?;
        let current: Option<i64> = sqlx::query_scalar(
            "SELECT revision FROM workflow_tool_registrations WHERE company_id=$1 AND name=$2 FOR UPDATE",
        )
        .bind(company.as_uuid())
        .bind(name.as_str())
        .fetch_optional(&mut *tx)
        .await?;
        match current {
            None => return Err(missing()),
            Some(current) if current != revision => return Err(conflict()),
            Some(_) => {}
        }
        sqlx::query("DELETE FROM workflow_tool_registrations WHERE company_id=$1 AND name=$2")
            .bind(company.as_uuid())
            .bind(name.as_str())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn workflow_tools(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
    ) -> AppResult<Vec<WorkflowToolRegistration>> {
        let mut tx = begin_http(self).await?;
        authority::authorize_company(&mut tx, company, actor).await?;
        let rows = sqlx::query_as::<_, RegistrationRow>(
            "SELECT name, workflow_id, version_id, params, revision FROM workflow_tool_registrations \
             WHERE company_id=$1 ORDER BY name LIMIT 257",
        )
        .bind(company.as_uuid())
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        if rows.len() > MAX_WORKFLOW_TOOLS {
            return Err(invalid());
        }
        rows.into_iter().map(RegistrationRow::decode).collect()
    }
}
