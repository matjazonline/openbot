use super::*;
use crate::application::workflow::binding::{ResourceDirectory, ResourceStatus, validate_resource};

/// HTTP connections, company MCP connections and the company's built-in messaging resource
/// (identified by the company itself) are the supported resource owners. Supported contracts
/// are the currently approved operation names; a ready resource still grants nothing by itself.
pub(super) async fn inspect_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    id: RuntimeResourceId,
) -> AppResult<Option<ResourceStatus>> {
    if id.as_uuid() == company.as_uuid() {
        // The caller already holds the company authority this resource is owned by.
        return message::messaging_status(company).map(Some);
    }
    if let Some(connection) = http::connection_on(tx, company, id.as_uuid()).await? {
        return http::status_on(tx, &connection).await.map(Some);
    }
    if let Some(status) = jev::status_on(tx, company, id.as_uuid()).await? {
        return Ok(Some(status));
    }
    match mcp::connection_on(tx, company, id.as_uuid()).await? {
        Some(connection) => mcp::status_on(tx, &connection).await.map(Some),
        None => Ok(None),
    }
}

/// The draft steps that use a selected tool, for naming them in a publication refusal (Plan07
/// 01B.2): steps whose literal `connection` is the selection's slot (and literal `tool`, when
/// they name one, its tool), or for an unslotted tool, whose literal `tool` is its name. A step
/// that picks it through an expression is not named. Empty when none is found.
pub(super) fn steps_using(source: &str, slot: Option<&str>, tool: &str) -> String {
    let Ok(source) = serde_json::from_str::<serde_json::Value>(source) else {
        return String::new();
    };
    let steps: Vec<&str> = source["steps"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, step)| {
            let with = &step["with"];
            match slot {
                Some(slot) => {
                    with["connection"]["literal"] == slot
                        && (with["tool"].is_null() || with["tool"]["literal"] == tool)
                }
                None => with["tool"]["literal"] == tool,
            }
        })
        .map(|(id, _)| id.as_str())
        .collect();
    match steps.as_slice() {
        [] => String::new(),
        [step] => format!(", used by step {step}"),
        steps => format!(", used by steps {}", steps.join(", ")),
    }
}

/// Who may use a runtime resource for a binding or a run's action (Plan07 decision e): an HTTP
/// or MCP connection belongs to whoever manages its unit, as its configuration does; the
/// company's messaging and JEV resources stay with root management.
pub(super) async fn authorize_use_on(
    db: &mut PgConnection,
    company: CompanyId,
    actor: WorkflowActor,
    id: Uuid,
) -> AppResult<()> {
    use crate::application::ports::unit_resources::UnitResourceKind;
    let kind: Option<String> = sqlx::query_scalar(
        "SELECT kind FROM (SELECT 'http' AS kind FROM workflow_http_connections \
         WHERE company_id=$1 AND id=$2 UNION ALL SELECT 'mcp' FROM company_mcp_connections \
         WHERE company_id=$1 AND id=$2 AND deleted_at IS NULL) AS owned LIMIT 1",
    )
    .bind(company.as_uuid())
    .bind(id)
    .fetch_optional(&mut *db)
    .await?;
    let kind = match kind.as_deref() {
        Some("http") => UnitResourceKind::HttpConnection,
        Some("mcp") => UnitResourceKind::McpConnection,
        _ => {
            return authority::authorize_company(db, company, actor)
                .await
                .map(|_| ());
        }
    };
    crate::adapters::persistence::unit_resources::authorize_resource_on(
        db, actor, company, kind, id,
    )
    .await
    .map(|_| ())
}

#[async_trait]
impl ResourceDirectory for PostgresPersistence {
    async fn inspect(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        id: RuntimeResourceId,
    ) -> AppResult<Option<ResourceStatus>> {
        let mut tx = self.pool.begin().await?;
        authorize_use_on(&mut tx, company, actor, id.as_uuid()).await?;
        let status = inspect_on(&mut tx, company, id).await?;
        tx.commit().await?;
        Ok(status)
    }
}

pub(super) struct ReadinessSelection<'a> {
    pub company: CompanyId,
    pub bundle: &'a PublishedBundle,
    pub resources:
        &'a std::collections::BTreeMap<crate::domain::workflow::ResourceName, RuntimeResourceId>,
    /// Where workflow-level connections must be reachable from (Plan07): the association's base
    /// department, the root when `None`.
    pub reach: Option<crate::entities::department::DepartmentId>,
    pub stage: crate::app_error::UnitCheckStage,
}

impl<'a> ReadinessSelection<'a> {
    pub(super) fn of(
        binding: &'a binding::ConfiguredBinding,
        reach: Option<crate::entities::department::DepartmentId>,
        stage: crate::app_error::UnitCheckStage,
    ) -> Self {
        Self {
            company: binding.company_id(),
            bundle: binding.bundle(),
            resources: binding.resources(),
            reach,
            stage,
        }
    }
}

/// The start of a run's reach for workflow-level connections (Plan07 decision d): the
/// association's base department, which equals the run's base read set; the root when none.
pub(super) async fn association_reach_on(
    tx: &mut PgConnection,
    company: CompanyId,
    association: crate::application::workflow::RelatedAssociation,
) -> AppResult<Option<crate::entities::department::DepartmentId>> {
    Ok(super::role_binding::context_on(tx, company, association)
        .await?
        .ok())
}

/// A run's reach once it exists: its base scope rows' own unit (the root when the chain was
/// empty), the same start admission used.
pub(super) async fn run_reach_on(
    tx: &mut PgConnection,
    company: CompanyId,
    run: crate::domain::workflow::RunId,
) -> AppResult<Option<crate::entities::department::DepartmentId>> {
    let unit: Option<Uuid> = sqlx::query_scalar(
        "SELECT unit_id FROM workflow_run_scopes WHERE company_id=$1 AND run_id=$2 \
         AND acting_principal_id IS NULL AND access='read' AND own LIMIT 1",
    )
    .bind(company.as_uuid())
    .bind(run.as_uuid())
    .fetch_optional(tx)
    .await?;
    Ok(unit.map(crate::entities::department::DepartmentId::new))
}

/// A slot's HTTP or MCP connection must belong to a unit in reach (Plan07 01B.3, D8).
async fn check_connection_unit_on(
    tx: &mut PgConnection,
    configuration: &ReadinessSelection<'_>,
    slot: &crate::domain::workflow::ResourceName,
    id: Uuid,
) -> AppResult<()> {
    use crate::application::ports::unit_resources::UnitResourceKind;
    let owner: Option<(String, bool)> = sqlx::query_as(
        "SELECT kind, unit_reaches($1, COALESCE($3, (SELECT id FROM departments \
            WHERE company_id=$1 AND is_root)), unit_id) FROM ( \
            SELECT 'http' AS kind, unit_id FROM workflow_http_connections \
                WHERE company_id=$1 AND id=$2 \
            UNION ALL SELECT 'mcp', unit_id FROM company_mcp_connections \
                WHERE company_id=$1 AND id=$2 AND unit_id IS NOT NULL) AS owner",
    )
    .bind(configuration.company.as_uuid())
    .bind(id)
    .bind(configuration.reach.map(|unit| unit.as_uuid()))
    .fetch_optional(&mut *tx)
    .await?;
    match owner {
        Some((kind, false)) => Err(AppError::ResourceOutsideUnit(
            crate::app_error::ResourceOutsideUnit {
                stage: configuration.stage,
                kind: if kind == "http" {
                    UnitResourceKind::HttpConnection
                } else {
                    UnitResourceKind::McpConnection
                },
                resource: id,
                name: slot.as_str().to_owned(),
            },
        )),
        _ => Ok(()),
    }
}

pub(super) async fn check_readiness(
    tx: &mut Transaction<'_, Postgres>,
    configuration: ReadinessSelection<'_>,
) -> AppResult<()> {
    let company = configuration.company;
    let definition = configuration.bundle.compiled().graph().definition();
    for tool in &configuration.bundle.snapshots().tools {
        let slot = match tool.kind() {
            ToolKind::LocalAgent => {
                if !crate::application::local_agents::workflow::is_trusted_snapshot(tool)? {
                    return Err(missing());
                }
                continue;
            }
            ToolKind::Message => {
                authority::builtin_message_tool(tool)?;
                continue;
            }
            ToolKind::Provisioning => {
                authority::builtin_provisioning_tool(tool)?;
                continue;
            }
            ToolKind::Escalation => {
                authority::builtin_escalation_tool(tool)?;
                continue;
            }
            ToolKind::Knowledge => {
                authority::builtin_knowledge_tool(tool)?;
                continue;
            }
            ToolKind::Memory => {
                authority::builtin_memory_tool(tx, tool, super::workflow_tools::ToolCheck::Run)
                    .await?;
                continue;
            }
            ToolKind::Http => {
                http::registered_on(
                    tx,
                    company,
                    tool,
                    http::ConnectionReach::From(configuration.reach),
                )
                .await?;
                continue;
            }
            // A frozen pin runs while its name is registered for the same workflow (P07-Q2).
            ToolKind::Workflow(_) => {
                super::workflow_tools::registered_on(
                    tx,
                    company,
                    tool,
                    super::workflow_tools::ToolCheck::Run,
                )
                .await?;
                continue;
            }
            ToolKind::Slot(slot) => slot,
        };
        let id = configuration
            .resources
            .get(slot)
            .ok_or_else(missing)?
            .as_uuid();
        match slot_kind(definition, slot) {
            Some("http") => {
                let connection = http::connection_on(tx, company, id)
                    .await?
                    .ok_or_else(missing)?;
                http::policy_on(tx, connection.company, connection.id, tool).await?;
            }
            Some("mcp") => {
                let connection = mcp::connection_on(tx, company, id)
                    .await?
                    .ok_or_else(missing)?;
                mcp::policy_on(tx, &connection, tool).await?;
            }
            _ => return Err(missing()),
        }
    }
    for requirement in &configuration
        .bundle
        .compiled()
        .graph()
        .definition()
        .resources
    {
        let id = *configuration
            .resources
            .get(&requirement.slot)
            .ok_or_else(missing)?;
        check_connection_unit_on(tx, &configuration, &requirement.slot, id.as_uuid()).await?;
        let status = inspect_on(tx, configuration.company, id)
            .await?
            .ok_or_else(missing)?;
        validate_resource(configuration.company, id, requirement, &status)?;
    }
    jev_binding_models(tx, &configuration).await
}

async fn jev_binding_models(
    tx: &mut Transaction<'_, Postgres>,
    configuration: &ReadinessSelection<'_>,
) -> AppResult<()> {
    for (slot, models) in crate::application::workflow::jev::required_models(configuration.bundle)?
    {
        let id = configuration.resources.get(&slot).ok_or_else(missing)?;
        let connection = jev::connection_on(tx, configuration.company, id.as_uuid())
            .await?
            .ok_or_else(jev::revoked)?;
        if !connection.enabled || !models.is_subset(&connection.models) {
            return Err(jev::revoked());
        }
    }
    Ok(())
}

/// The declared resource kind of a tool's slot in this exact workflow definition.
pub(super) fn slot_kind<'a>(
    definition: &'a crate::domain::workflow::WorkflowDefinition,
    slot: &crate::domain::workflow::ResourceName,
) -> Option<&'a str> {
    definition
        .resources
        .iter()
        .find(|resource| &resource.slot == slot)
        .map(|resource| resource.kind.as_str())
}
