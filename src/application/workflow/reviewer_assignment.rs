//! Current reviewer membership and its immutable history, separate from the proposal revision.
use super::decisions::DecisionId;
use super::people_resolution::{PeopleResolutionRevision, checked_human_revision};
use super::reviewers::ReviewerSelector;
use crate::app_error::{AppError, AppResult};
use crate::domain::workflow::{CompanyId, RunId, StepId, VersionId};
use crate::entities::agent_capability::DependencyManifestId;
use crate::entities::department::{DepartmentId, DepartmentRole};
use crate::entities::notification::NotificationEventId;
use crate::entities::transport::PrincipalId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_REVIEWER_ASSIGNMENT_BYTES: usize = 32 * 1024;
pub const MAX_REVIEWER_LABEL_BYTES: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct ReviewerAssignmentRevision(u64);

impl TryFrom<u64> for ReviewerAssignmentRevision {
    type Error = &'static str;
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        checked_human_revision(value).map(Self)
    }
}

impl From<ReviewerAssignmentRevision> for u64 {
    fn from(value: ReviewerAssignmentRevision) -> Self {
        value.0
    }
}

impl ReviewerAssignmentRevision {
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub enum SelectedPeopleRevision {
    Admission,
    Reassigned(PeopleResolutionRevision),
}

impl TryFrom<u64> for SelectedPeopleRevision {
    type Error = &'static str;
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        if value == 0 {
            Ok(Self::Admission)
        } else {
            PeopleResolutionRevision::try_from(value).map(Self::Reassigned)
        }
    }
}

impl From<SelectedPeopleRevision> for u64 {
    fn from(value: SelectedPeopleRevision) -> Self {
        match value {
            SelectedPeopleRevision::Admission => 0,
            SelectedPeopleRevision::Reassigned(value) => value.get(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewerSelection {
    pub principals: Vec<PrincipalId>,
    pub roles: Vec<DepartmentRole>,
}

impl ReviewerSelection {
    /// Retain authored order for existing decisions; canonical documents sort their own facts.
    pub fn from_selectors(selectors: &[ReviewerSelector]) -> Self {
        let mut principals = Vec::with_capacity(super::registry::MAX_REVIEWERS);
        let mut roles = Vec::with_capacity(DepartmentRole::ALL.len());
        for selector in selectors {
            match selector {
                ReviewerSelector::Principal(id) if !principals.contains(id) => principals.push(*id),
                ReviewerSelector::Role(role) if !roles.contains(role) => roles.push(*role),
                _ => {}
            }
        }
        Self { principals, roles }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssignmentRoleReference {
    pub role: DepartmentRole,
    pub resolution_revision: SelectedPeopleRevision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssignmentReviewer {
    pub principal_id: PrincipalId,
    pub ordinal: u8,
    pub reviewer_label: String,
    pub notice_event_id: NotificationEventId,
    /// An unchanged member retains its original notice proof across later role captures.
    pub notice_assignment_revision: ReviewerAssignmentRevision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewerAssignmentRefusal {
    RoleUnavailable,
    TooMany,
    ReviewerUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewerAssignmentFormat {
    #[serde(rename = "workflow-reviewer-assignment-v1")]
    V1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewerAssignment {
    pub format: ReviewerAssignmentFormat,
    pub company_id: CompanyId,
    #[serde(with = "decision_identity")]
    pub decision_id: DecisionId,
    pub root_run_id: RunId,
    pub manifest_id: DependencyManifestId,
    pub version_id: VersionId,
    pub step: StepId,
    pub context_department_id: DepartmentId,
    pub assignment_revision: ReviewerAssignmentRevision,
    pub roles: Vec<AssignmentRoleReference>,
    pub reviewers: Vec<AssignmentReviewer>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub refusal: Option<ReviewerAssignmentRefusal>,
}

impl ReviewerAssignment {
    pub fn validate(&self) -> AppResult<()> {
        if self.roles.is_empty()
            || self.roles.len() > DepartmentRole::ALL.len()
            || self
                .roles
                .iter()
                .map(|role| role.role)
                .collect::<BTreeSet<_>>()
                .len()
                != self.roles.len()
            || self.reviewers.len() > super::registry::MAX_REVIEWERS
            || (self.refusal.is_some() != self.reviewers.is_empty())
        {
            return Err(invalid());
        }
        let mut principals = BTreeSet::new();
        let mut ordinals = BTreeSet::new();
        let mut events = BTreeSet::new();
        for reviewer in &self.reviewers {
            if !principals.insert(reviewer.principal_id)
                || !ordinals.insert(reviewer.ordinal)
                || !events.insert(reviewer.notice_event_id)
                || usize::from(reviewer.ordinal) >= super::registry::MAX_REVIEWERS
                || reviewer.reviewer_label.len() > MAX_REVIEWER_LABEL_BYTES
                || reviewer.notice_assignment_revision > self.assignment_revision
            {
                return Err(invalid());
            }
        }
        Ok(())
    }

    pub fn decode(bytes: &[u8]) -> AppResult<Self> {
        if bytes.len() > MAX_REVIEWER_ASSIGNMENT_BYTES {
            return Err(invalid());
        }
        let value: Self = serde_json::from_slice(bytes).map_err(|_| invalid())?;
        value.validate()?;
        Ok(value)
    }

    pub fn canonical_content(&self) -> AppResult<Vec<u8>> {
        self.validate()?;
        let mut value = self.clone();
        value.roles.sort_unstable_by_key(|role| role.role);
        value
            .reviewers
            .sort_unstable_by_key(|reviewer| reviewer.ordinal);
        let value = serde_json::to_value(value).map_err(|_| invalid())?;
        let bytes = serde_json::to_vec(&value).map_err(|_| invalid())?;
        if bytes.len() > MAX_REVIEWER_ASSIGNMENT_BYTES {
            return Err(invalid());
        }
        Ok(bytes)
    }
}

mod decision_identity {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        value: &DecisionId,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        value.0.serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        decoder: D,
    ) -> Result<DecisionId, D::Error> {
        uuid::Uuid::deserialize(decoder).map(DecisionId)
    }
}

fn invalid() -> AppError {
    AppError::BadRequest("Invalid canonical reviewer assignment".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn assignment() -> ReviewerAssignment {
        ReviewerAssignment {
            format: ReviewerAssignmentFormat::V1,
            company_id: CompanyId::new(Uuid::new_v4()),
            decision_id: DecisionId(Uuid::new_v4()),
            root_run_id: RunId::new(Uuid::new_v4()),
            manifest_id: DependencyManifestId::new(Uuid::new_v4()),
            version_id: VersionId::new(Uuid::new_v4()),
            step: StepId::parse("review").unwrap(),
            context_department_id: DepartmentId::new(Uuid::new_v4()),
            assignment_revision: ReviewerAssignmentRevision::try_from(2).unwrap(),
            roles: vec![AssignmentRoleReference {
                role: DepartmentRole::Reviewer,
                resolution_revision: SelectedPeopleRevision::Reassigned(
                    PeopleResolutionRevision::try_from(1).unwrap(),
                ),
            }],
            reviewers: (0..2)
                .map(|ordinal| AssignmentReviewer {
                    principal_id: PrincipalId::new(Uuid::new_v4()),
                    ordinal,
                    reviewer_label: format!("Reviewer {ordinal}"),
                    notice_event_id: NotificationEventId::random(),
                    notice_assignment_revision: ReviewerAssignmentRevision::try_from(
                        u64::from(ordinal) + 1,
                    )
                    .unwrap(),
                })
                .collect(),
            refusal: None,
        }
    }

    #[test]
    fn workflow_reviewer_follow_assignment_preserves_notice_history_and_rejects_future_proof() {
        let mut value = assignment();
        let canonical = value.canonical_content().unwrap();
        value.reviewers.reverse();
        assert_eq!(value.canonical_content().unwrap(), canonical);
        let restored = ReviewerAssignment::decode(&value.canonical_content().unwrap()).unwrap();
        assert_eq!(restored.reviewers[0].notice_assignment_revision.get(), 1);
        assert_eq!(restored.canonical_content().unwrap(), canonical);
        value.reviewers[0].notice_assignment_revision =
            ReviewerAssignmentRevision::try_from(3).unwrap();
        assert!(value.validate().is_err());
        let mut value = assignment();
        value.reviewers.clear();
        assert!(value.validate().is_err());
        value.refusal = Some(ReviewerAssignmentRefusal::RoleUnavailable);
        assert!(
            ReviewerAssignment::decode(&value.canonical_content().unwrap())
                .unwrap()
                .reviewers
                .is_empty()
        );
    }

    #[test]
    fn workflow_reviewer_follow_assignment_rejects_ambiguous_members_and_implicit_fields() {
        let mut value = assignment();
        value.reviewers[1].ordinal = value.reviewers[0].ordinal;
        assert!(value.validate().is_err());
        let mut value = assignment();
        value.reviewers[1].principal_id = value.reviewers[0].principal_id;
        assert!(value.validate().is_err());
        let mut value = assignment();
        value.reviewers[1].notice_event_id = value.reviewers[0].notice_event_id;
        assert!(value.validate().is_err());
        let mut value = assignment();
        value.roles.push(value.roles[0].clone());
        assert!(value.validate().is_err());
        let mut source = serde_json::to_value(assignment()).unwrap();
        source.as_object_mut().unwrap().remove("refusal");
        assert!(ReviewerAssignment::decode(&serde_json::to_vec(&source).unwrap()).is_err());
        assert!(ReviewerAssignmentRevision::try_from(0).is_err());
        assert!(ReviewerAssignmentRevision::try_from(i64::MAX as u64 + 1).is_err());
    }
}
