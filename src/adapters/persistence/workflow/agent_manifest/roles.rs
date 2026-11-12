use super::*;
use crate::application::agent::{RolePrincipalSelection, RoleResolution};
use crate::application::workflow::publication::{RoleReference, RoleUseMode};
use crate::application::workflow::reviewers::{
    ReviewerSelector, approval_reviewers, literal_reviewers, parse_reviewers,
};
use crate::application::workflow::roles::{RoleDepartment, RoleHolder, RoleNeed, resolve_role};
use crate::entities::department::{
    DepartmentId, DepartmentRole, MAX_DEPARTMENT_DEPTH, RoleAdmissionRefusal, RoleRefusalKind,
};
use crate::entities::participant::PrincipalKind;
use crate::entities::transport::PrincipalId;

pub(super) struct RoleFacts {
    chain: Vec<RoleDepartment>,
    agents: BTreeMap<PrincipalId, AgentId>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct RoleCaptureKey {
    version: VersionId,
    step: StepId,
    role: DepartmentRole,
}

pub(super) struct RoleCapture {
    reference: RoleReference,
    department: DepartmentId,
    principals: Vec<PrincipalId>,
    agent: Option<AgentId>,
}

pub(super) fn role_error(reference: &RoleReference, kind: RoleRefusalKind) -> AppError {
    AppError::WorkflowRole(RoleAdmissionRefusal::new(
        reference.step.clone(),
        reference.role,
        kind,
    ))
}

#[derive(sqlx::FromRow)]
struct HolderRow {
    department: Uuid,
    role: String,
    principal: Option<Uuid>,
    kind: Option<String>,
    agent: Option<Uuid>,
}

async fn load_facts_on(
    tx: &mut PgConnection,
    company: CompanyId,
    association: RelatedAssociation,
    reference: &RoleReference,
) -> AppResult<RoleFacts> {
    let channel = match association {
        RelatedAssociation::Company => {
            return Err(role_error(reference, RoleRefusalKind::MissingDepartment));
        }
        RelatedAssociation::Channel(channel)
        | RelatedAssociation::Thread {
            channel_id: channel,
            ..
        } => channel,
    };
    let department: Option<Uuid> =
        sqlx::query_scalar("SELECT department_id FROM channels WHERE company_id=$1 AND id=$2")
            .bind(company.as_uuid())
            .bind(channel.as_uuid())
            .fetch_optional(&mut *tx)
            .await?
            .flatten();
    let department =
        department.ok_or_else(|| role_error(reference, RoleRefusalKind::MissingDepartment))?;
    load_context_on(tx, company, DepartmentId::new(department)).await
}

async fn load_context_on(
    tx: &mut PgConnection,
    company: CompanyId,
    department: DepartmentId,
) -> AppResult<RoleFacts> {
    let vocabulary: Vec<_> = DepartmentRole::ALL
        .into_iter()
        .map(|role| role.as_str().to_owned())
        .collect();
    let rows = sqlx::query_as::<_, HolderRow>(
        "WITH RECURSIVE chain AS ( \
           SELECT department.id,department.parent_id,1 AS depth FROM departments AS department \
           WHERE department.company_id=$1 AND department.id=$2 \
           UNION ALL SELECT parent.id,parent.parent_id,chain.depth+1 FROM departments AS parent \
           JOIN chain ON chain.parent_id=parent.id WHERE parent.company_id=$1 AND chain.depth<$3 \
         ) SELECT chain.id AS department,vocabulary.role,holder.principal_id AS principal,principal.kind,principal.agent_id AS agent \
         FROM chain CROSS JOIN unnest($5::text[]) AS vocabulary(role) \
         LEFT JOIN LATERAL (SELECT principal_id FROM department_roles \
             WHERE company_id=$1 AND department_id=chain.id AND role=vocabulary.role \
             ORDER BY principal_id LIMIT $4) AS holder ON true \
         LEFT JOIN principals AS principal ON principal.company_id=$1 AND principal.id=holder.principal_id \
         ORDER BY chain.depth,vocabulary.role,holder.principal_id",
    )
    .bind(company.as_uuid()).bind(department.as_uuid())
    .bind((MAX_DEPARTMENT_DEPTH + 1) as i32)
    .bind((crate::domain::workflow::TokenAllowance::MAX_REVIEWERS + 1) as i64)
    .bind(vocabulary)
    .fetch_all(tx).await?;
    facts_from_rows(rows)
}

fn facts_from_rows(rows: Vec<HolderRow>) -> AppResult<RoleFacts> {
    let mut facts = RoleFacts {
        chain: Vec::new(),
        agents: BTreeMap::new(),
    };
    for row in rows {
        if facts
            .chain
            .last()
            .is_none_or(|entry| entry.department.as_uuid() != row.department)
        {
            facts.chain.push(RoleDepartment {
                department: DepartmentId::new(row.department),
                holders: Vec::new(),
            });
        }
        let Some(principal) = row.principal else {
            continue;
        };
        let kind = match row.kind.as_deref() {
            Some("agent") => PrincipalKind::Agent,
            Some("person") => PrincipalKind::Person,
            Some("external") => PrincipalKind::External,
            Some("system") => PrincipalKind::System,
            _ => return Err(invalid()),
        };
        let role = row.role.parse::<DepartmentRole>().map_err(|_| invalid())?;
        let principal = PrincipalId::new(principal);
        if let Some(agent) = row.agent {
            facts.agents.insert(principal, AgentId::new(agent));
        }
        facts
            .chain
            .last_mut()
            .ok_or_else(invalid)?
            .holders
            .push(RoleHolder {
                role,
                principal,
                kind,
            });
    }
    Ok(facts)
}

impl Closure {
    pub(super) fn role_agent_error(&self, agent: AgentId, error: AppError) -> AppError {
        if matches!(
            &error,
            AppError::BadRequest(_) | AppError::NotFound(_) | AppError::Forbidden(_)
        ) && let Some(capture) = self
            .roles
            .values()
            .find(|capture| capture.agent == Some(agent))
        {
            return role_error(&capture.reference, RoleRefusalKind::Unavailable);
        }
        error
    }

    pub(super) async fn role_edges_on(
        &mut self,
        tx: &mut Transaction<'_, Postgres>,
        bundle: &PublishedBundle,
    ) -> AppResult<Vec<Node>> {
        let references = bundle.compiled().role_references();
        let Some(first) = references.first() else {
            return Ok(Vec::new());
        };
        if self.role_facts.is_none() {
            self.role_facts = Some(load_facts_on(tx, self.company, self.association, first).await?);
        }
        let facts = self.role_facts.as_ref().ok_or_else(invalid)?;
        let version = bundle.compiled().graph().definition().version_id;
        let mut edges = Vec::with_capacity(references.len());
        let mut reviewers = BTreeMap::<StepId, std::collections::BTreeSet<PrincipalId>>::new();
        for reference in references {
            let need = if reference.mode == RoleUseMode::People {
                RoleNeed::People
            } else {
                RoleNeed::Agent
            };
            let resolved = resolve_role(&facts.chain, reference.role, need)
                .map_err(|kind| role_error(reference, kind))?;
            let agent = if let Some(mode) = reference.mode.agent_mode() {
                let principal = resolved.principals.first().ok_or_else(invalid)?;
                let agent = *facts
                    .agents
                    .get(principal)
                    .ok_or_else(|| role_error(reference, RoleRefusalKind::WrongKind))?;
                edges.push(Node::Agent(agent, mode));
                Some(agent)
            } else {
                let selected = reviewers.entry(reference.step.clone()).or_default();
                selected.extend(resolved.principals.iter().copied());
                selected.extend(known_principals(bundle, &reference.step)?);
                if selected.len() > crate::domain::workflow::TokenAllowance::MAX_REVIEWERS {
                    return Err(role_error(reference, RoleRefusalKind::TooMany));
                }
                None
            };
            self.roles.insert(
                RoleCaptureKey {
                    version,
                    step: reference.step.clone(),
                    role: reference.role,
                },
                RoleCapture {
                    reference: reference.clone(),
                    department: resolved.department,
                    principals: resolved.principals,
                    agent,
                },
            );
        }
        Ok(edges)
    }

    pub(super) async fn finish_roles_on(
        &self,
        tx: &mut PgConnection,
    ) -> AppResult<Vec<RoleResolution>> {
        // All selected agent SHARE locks already exist. Principal locks follow agents, ordered
        // by principal ID, matching retirement rather than introducing the reverse edge.
        let principals: BTreeMap<_, _> = self
            .roles
            .values()
            .flat_map(|capture| {
                capture
                    .principals
                    .iter()
                    .map(move |principal| (*principal, capture))
            })
            .collect();
        for (principal, capture) in principals {
            self.within_bound()?;
            let actual: Option<(String, Option<Uuid>)> = sqlx::query_as(
                "SELECT kind,agent_id FROM principals WHERE company_id=$1 AND id=$2 FOR KEY SHARE",
            )
            .bind(self.company.as_uuid())
            .bind(principal.as_uuid())
            .fetch_optional(&mut *tx)
            .await?;
            let expected = capture.agent.map_or_else(
                || ("person".into(), None),
                |agent| ("agent".into(), Some(agent.as_uuid())),
            );
            if actual != Some(expected) {
                return Err(role_error(&capture.reference, RoleRefusalKind::WrongKind));
            }
        }
        self.roles
            .iter()
            .map(|(key, capture)| {
                let revision = capture
                    .agent
                    .map(|agent| {
                        self.agents.get(&agent).copied().ok_or_else(|| {
                            role_error(&capture.reference, RoleRefusalKind::Unavailable)
                        })
                    })
                    .transpose()?;
                Ok(RoleResolution {
                    version_id: key.version,
                    step: capture.reference.step.clone(),
                    role: capture.reference.role,
                    department_id: capture.department,
                    principals: capture
                        .principals
                        .iter()
                        .map(|principal| RolePrincipalSelection {
                            principal_id: *principal,
                            agent_id: capture.agent,
                            revision_id: revision,
                        })
                        .collect(),
                })
            })
            .collect()
    }
}

fn known_principals(bundle: &PublishedBundle, step: &StepId) -> AppResult<Vec<PrincipalId>> {
    let definition = &bundle.compiled().graph().definition().steps[step];
    let binding = if definition.step_type.as_str() == "decision.human" {
        definition.inputs.get("reviewers").cloned()
    } else {
        definition
            .inputs
            .get("approval")
            .and_then(approval_reviewers)
    };
    let Some(value) = binding.as_ref().and_then(literal_reviewers) else {
        return Ok(Vec::new());
    };
    Ok(parse_reviewers(&value)?
        .into_iter()
        .filter_map(|selector| match selector {
            ReviewerSelector::Principal(id) => Some(id),
            ReviewerSelector::Role(_) => None,
        })
        .collect())
}

/// Admission and followed decisions share the same ancestry facts and cardinality rules.
pub(in crate::adapters::persistence::workflow) async fn resolve_people_on(
    db: &mut PgConnection,
    company: CompanyId,
    department: DepartmentId,
    role: DepartmentRole,
) -> AppResult<Result<crate::application::workflow::roles::ResolvedRole, RoleRefusalKind>> {
    let facts = load_context_on(db, company, department).await?;
    if facts.chain.is_empty() {
        return Ok(Err(RoleRefusalKind::MissingDepartment));
    }
    Ok(resolve_role(&facts.chain, role, RoleNeed::People))
}

pub(in crate::adapters::persistence::workflow) async fn notification_recipients_on(
    db: &mut PgConnection,
    company: CompanyId,
    department: DepartmentId,
    role: DepartmentRole,
) -> AppResult<crate::application::workflow::escalation::NotificationRecipients> {
    let facts = load_context_on(db, company, department).await?;
    if facts.chain.is_empty() {
        return Ok(crate::application::workflow::escalation::NotificationRecipients::Refused {
            department:None,
            reason:crate::application::workflow::escalation::NotificationRoleRefusal::MissingContext,
        });
    }
    Ok(
        crate::application::workflow::escalation::resolve_notification_recipients(
            &facts.chain,
            role,
        ),
    )
}

pub(in crate::adapters::persistence::workflow) async fn notification_department_recipients_on(
    db: &mut PgConnection,
    company: CompanyId,
    department: DepartmentId,
    role: DepartmentRole,
) -> AppResult<crate::application::workflow::escalation::NotificationRecipients> {
    let facts = load_context_on(db, company, department).await?;
    if facts.chain.is_empty() {
        return Ok(crate::application::workflow::escalation::NotificationRecipients::Refused {
            department:None,reason:crate::application::workflow::escalation::NotificationRoleRefusal::MissingContext,
        });
    }
    Ok(
        crate::application::workflow::escalation::resolve_notification_recipients(
            &facts.chain[..1],
            role,
        ),
    )
}
