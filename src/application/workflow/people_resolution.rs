//! An immutable canonical People resolution. Admission's agent selections remain revision zero.
use crate::app_error::{AppError, AppResult};
use crate::domain::workflow::{CompanyId, RunId, StepId, VersionId};
use crate::entities::agent_capability::DependencyManifestId;
use crate::entities::department::{DepartmentId, DepartmentRole, RoleRefusalKind};
use crate::entities::transport::PrincipalId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_PEOPLE_RESOLUTION_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct PeopleResolutionRevision(u64);

impl TryFrom<u64> for PeopleResolutionRevision {
    type Error = &'static str;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        checked_human_revision(value).map(Self)
    }
}

pub(super) fn checked_human_revision(value: u64) -> Result<u64, &'static str> {
    if value == 0 || value > i64::MAX as u64 {
        return Err("A human assignment revision must be between 1 and i64::MAX");
    }
    Ok(value)
}

impl From<PeopleResolutionRevision> for u64 {
    fn from(value: PeopleResolutionRevision) -> Self {
        value.0
    }
}

impl PeopleResolutionRevision {
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PeopleResolutionFormat {
    #[serde(rename = "workflow-human-role-resolution-v1")]
    V1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeopleResolution {
    pub format: PeopleResolutionFormat,
    pub company_id: CompanyId,
    pub root_run_id: RunId,
    pub manifest_id: DependencyManifestId,
    pub version_id: VersionId,
    pub step: StepId,
    pub role: DepartmentRole,
    pub resolution_revision: PeopleResolutionRevision,
    pub context_department_id: DepartmentId,
    #[serde(deserialize_with = "Option::deserialize")]
    pub department_id: Option<DepartmentId>,
    pub principals: Vec<PrincipalId>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub refusal: Option<RoleRefusalKind>,
}

impl PeopleResolution {
    pub fn validate(&self) -> AppResult<()> {
        if self.principals.len() > super::registry::MAX_REVIEWERS
            || self
                .principals
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                .len()
                != self.principals.len()
        {
            return Err(invalid());
        }
        match self.refusal {
            None if self.principals.is_empty() || self.department_id.is_none() => Err(invalid()),
            Some(_) if !self.principals.is_empty() => Err(invalid()),
            _ => Ok(()),
        }
    }

    pub fn decode(bytes: &[u8]) -> AppResult<Self> {
        if bytes.len() > MAX_PEOPLE_RESOLUTION_BYTES {
            return Err(invalid());
        }
        let value: Self = serde_json::from_slice(bytes).map_err(|_| invalid())?;
        value.validate()?;
        Ok(value)
    }

    pub fn canonical_content(&self) -> AppResult<Vec<u8>> {
        self.validate()?;
        let mut value = self.clone();
        value.principals.sort_unstable();
        // Match the ordinary manifest's sorted object representation and identity encoding.
        let value = serde_json::to_value(value).map_err(|_| invalid())?;
        let bytes = serde_json::to_vec(&value).map_err(|_| invalid())?;
        if bytes.len() > MAX_PEOPLE_RESOLUTION_BYTES {
            return Err(invalid());
        }
        Ok(bytes)
    }
}

fn invalid() -> AppError {
    AppError::BadRequest("Invalid canonical People role resolution".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn resolution() -> PeopleResolution {
        PeopleResolution {
            format: PeopleResolutionFormat::V1,
            company_id: CompanyId::new(Uuid::new_v4()),
            root_run_id: RunId::new(Uuid::new_v4()),
            manifest_id: DependencyManifestId::new(Uuid::new_v4()),
            version_id: VersionId::new(Uuid::new_v4()),
            step: StepId::parse("review").unwrap(),
            role: DepartmentRole::Reviewer,
            resolution_revision: PeopleResolutionRevision::try_from(1).unwrap(),
            context_department_id: DepartmentId::new(Uuid::new_v4()),
            department_id: Some(DepartmentId::new(Uuid::new_v4())),
            principals: vec![
                PrincipalId::new(Uuid::new_v4()),
                PrincipalId::new(Uuid::new_v4()),
            ],
            refusal: None,
        }
    }

    #[test]
    fn workflow_reviewer_follow_canonical_people_sets_are_stable_and_explicitly_empty() {
        let mut value = resolution();
        let canonical = value.canonical_content().unwrap();
        value.principals.reverse();
        assert_eq!(value.canonical_content().unwrap(), canonical);
        let restored = PeopleResolution::decode(&value.canonical_content().unwrap()).unwrap();
        assert_eq!(restored.canonical_content().unwrap(), canonical);
        value.principals.clear();
        assert!(value.canonical_content().is_err());
        value.refusal = Some(RoleRefusalKind::NoHolder);
        value.department_id = None;
        let empty = value.canonical_content().unwrap();
        let restored = PeopleResolution::decode(&empty).unwrap();
        assert!(restored.principals.is_empty());
        assert_eq!(restored.refusal, Some(RoleRefusalKind::NoHolder));
        assert_ne!(restored.canonical_content().unwrap(), canonical);
    }

    #[test]
    fn workflow_reviewer_follow_canonical_people_reject_partial_or_ambiguous_documents() {
        let mut value = resolution();
        value.refusal = Some(RoleRefusalKind::WrongKind);
        assert!(value.validate().is_err());
        value.refusal = None;
        value.principals.push(value.principals[0]);
        assert!(value.validate().is_err());
        value.principals = (0..17).map(|_| PrincipalId::new(Uuid::new_v4())).collect();
        assert!(value.validate().is_err());
        let value = resolution();
        for field in ["department_id", "refusal"] {
            let mut document = serde_json::to_value(&value).unwrap();
            document.as_object_mut().unwrap().remove(field);
            assert!(
                PeopleResolution::decode(&serde_json::to_vec(&document).unwrap()).is_err(),
                "{field}"
            );
        }
        let mut document = serde_json::to_value(value).unwrap();
        document["agent_id"] = serde_json::json!(Uuid::new_v4());
        assert!(PeopleResolution::decode(&serde_json::to_vec(&document).unwrap()).is_err());
        document.as_object_mut().unwrap().remove("agent_id");
        document.as_object_mut().unwrap().remove("principals");
        assert!(PeopleResolution::decode(&serde_json::to_vec(&document).unwrap()).is_err());
        assert!(PeopleResolutionRevision::try_from(0).is_err());
        assert!(PeopleResolutionRevision::try_from(i64::MAX as u64 + 1).is_err());
    }
}
