use super::*;
use crate::application::workflow::roles::ResolvedRole;

pub(super) struct ResolvedPeople {
    pub references: Vec<AssignmentRoleReference>,
    pub principals: BTreeSet<PrincipalId>,
    pub unavailable: bool,
}

pub(super) async fn resolve_on(
    db: &mut PgConnection,
    current: &FollowDecision,
    selection: &ReviewerSelection,
    previous: Option<&ReviewerAssignment>,
) -> AppResult<ResolvedPeople> {
    let step = StepId::parse(&current.step).map_err(|_| invalid())?;
    let mut resolved = ResolvedPeople {
        references: Vec::new(),
        principals: BTreeSet::new(),
        unavailable: false,
    };
    let mut roles = selection.roles.clone();
    roles.sort_unstable();
    for role in roles {
        let live = agent_manifest::resolve_people_on(
            db,
            current.company(),
            DepartmentId::new(current.context_department_id),
            role,
        )
        .await?;
        let before = previous
            .and_then(|assignment| {
                assignment
                    .roles
                    .iter()
                    .find(|reference| reference.role == role)
            })
            .map(|reference| reference.resolution_revision)
            .unwrap_or(SelectedPeopleRevision::Admission);
        let key = people_resolutions::ResolutionKey {
            company: current.company(),
            root: RunId::new(current.root_run_id),
            manifest: DependencyManifestId::new(current.manifest_id),
            version: VersionId::new(current.version_id),
            step: &step,
            role,
        };
        let captured = selected_on(db, &key, before).await?;
        let revision = if same_resolution(&captured, &live) {
            before
        } else {
            let revision = people_resolutions::next_revision_on(db, &key).await?;
            let value = later_resolution(current, &key, revision, &live);
            people_resolutions::persist_on(db, &value).await?;
            SelectedPeopleRevision::Reassigned(revision)
        };
        resolved.references.push(AssignmentRoleReference {
            role,
            resolution_revision: revision,
        });
        match live {
            Ok(value) => resolved.principals.extend(value.principals),
            Err(_) => resolved.unavailable = true,
        }
    }
    Ok(resolved)
}

fn later_resolution(
    current: &FollowDecision,
    key: &people_resolutions::ResolutionKey<'_>,
    revision: crate::application::workflow::people_resolution::PeopleResolutionRevision,
    live: &Result<ResolvedRole, RoleRefusalKind>,
) -> PeopleResolution {
    let mut value = PeopleResolution {
        format: PeopleResolutionFormat::V1,
        company_id: current.company(),
        root_run_id: key.root,
        manifest_id: key.manifest,
        version_id: key.version,
        step: key.step.clone(),
        role: key.role,
        resolution_revision: revision,
        context_department_id: DepartmentId::new(current.context_department_id),
        department_id: None,
        principals: Vec::new(),
        refusal: None,
    };
    match live {
        Ok(resolved) => {
            value.department_id = Some(resolved.department);
            value.principals = resolved.principals.clone();
        }
        Err(refusal) => value.refusal = Some(*refusal),
    }
    value
}

struct SelectedPeople {
    department: Option<DepartmentId>,
    principals: Vec<PrincipalId>,
    refusal: Option<RoleRefusalKind>,
}

#[derive(sqlx::FromRow)]
struct AdmissionMember {
    department_id: Uuid,
    principal_id: Uuid,
}

async fn selected_on(
    db: &mut PgConnection,
    key: &people_resolutions::ResolutionKey<'_>,
    revision: SelectedPeopleRevision,
) -> AppResult<SelectedPeople> {
    match revision {
        SelectedPeopleRevision::Admission => {
            let rows:Vec<AdmissionMember>=sqlx::query_as(
                "SELECT department_id,principal_id FROM workflow_run_role_bindings WHERE company_id=$1 AND run_id=$2 \
                 AND manifest_id=$3 AND version_id=$4 AND step=$5 AND role=$6 AND resolution_revision=0 \
                 AND agent_id IS NULL AND agent_revision_id IS NULL ORDER BY principal_id LIMIT 17")
                .bind(key.company.as_uuid()).bind(key.root.as_uuid()).bind(key.manifest.as_uuid())
                .bind(key.version.as_uuid()).bind(key.step.as_str()).bind(key.role.as_str()).fetch_all(db).await?;
            if rows.is_empty() || rows.len() > registry::MAX_REVIEWERS {
                return Err(invalid());
            }
            Ok(SelectedPeople {
                department: Some(DepartmentId::new(rows[0].department_id)),
                principals: rows
                    .iter()
                    .map(|row| PrincipalId::new(row.principal_id))
                    .collect(),
                refusal: None,
            })
        }
        SelectedPeopleRevision::Reassigned(revision) => {
            let value = people_resolutions::read_on(db, key, revision).await?;
            Ok(SelectedPeople {
                department: value.department_id,
                principals: value.principals,
                refusal: value.refusal,
            })
        }
    }
}

fn same_resolution(before: &SelectedPeople, live: &Result<ResolvedRole, RoleRefusalKind>) -> bool {
    match live {
        Ok(value) => {
            before.refusal.is_none()
                && before.department == Some(value.department)
                && before.principals == value.principals
        }
        Err(refusal) => before.principals.is_empty() && before.refusal == Some(*refusal),
    }
}
