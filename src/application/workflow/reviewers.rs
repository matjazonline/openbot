//! Authored reviewer identities. Expansion uses an admitted run's sealed role members.
use crate::app_error::{AppError, AppResult};
use crate::domain::workflow::Binding;
use crate::entities::department::DepartmentRole;
use crate::entities::transport::PrincipalId;
use serde_json::Value;
use uuid::Uuid;

pub const MAX_REVIEWER_SELECTORS: usize =
    super::registry::MAX_REVIEWERS + DepartmentRole::ALL.len();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewerSelector {
    Principal(PrincipalId),
    Role(DepartmentRole),
}

pub fn parse_reviewers(value: &Value) -> AppResult<Vec<ReviewerSelector>> {
    let invalid = || {
        AppError::BadRequest(
            "Choose person principals or literal department roles as reviewers".into(),
        )
    };
    let items = value.as_array().ok_or_else(invalid)?;
    if items.is_empty() || items.len() > MAX_REVIEWER_SELECTORS {
        return Err(invalid());
    }
    let mut selected = Vec::with_capacity(items.len());
    for item in items {
        let selector = if let Some(role) = item.as_str() {
            ReviewerSelector::Role(role.parse().map_err(|_| invalid())?)
        } else {
            let fields = item.as_object().ok_or_else(invalid)?;
            if fields.len() != 2 || fields.get("kind").and_then(Value::as_str) != Some("principal")
            {
                return Err(invalid());
            }
            let id = fields
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| Uuid::parse_str(id).ok())
                .filter(|id| !id.is_nil())
                .ok_or_else(invalid)?;
            ReviewerSelector::Principal(PrincipalId::new(id))
        };
        if !selected.contains(&selector) {
            selected.push(selector);
        }
    }
    if selected
        .iter()
        .filter(|item| matches!(item, ReviewerSelector::Principal(_)))
        .count()
        > super::registry::MAX_REVIEWERS
    {
        return Err(invalid());
    }
    Ok(selected)
}

/// Only fully authored arrays can declare role requirements. Parameters may select principals.
pub fn literal_reviewers(binding: &Binding) -> Option<Value> {
    match binding {
        Binding::Literal(value) => Some(value.clone()),
        Binding::Array(items) => items
            .iter()
            .map(literal_value)
            .collect::<Option<Vec<_>>>()
            .map(Value::Array),
        _ => None,
    }
}

fn literal_value(binding: &Binding) -> Option<Value> {
    match binding {
        Binding::Literal(value) => Some(value.clone()),
        Binding::Object(fields) => fields
            .iter()
            .map(|(key, binding)| Some((key.clone(), literal_value(binding)?)))
            .collect::<Option<serde_json::Map<_, _>>>()
            .map(Value::Object),
        _ => None,
    }
}

pub fn approval_reviewers(binding: &Binding) -> Option<Binding> {
    match binding {
        Binding::Literal(value) => value.get("reviewers").cloned().map(Binding::Literal),
        Binding::Object(fields) => fields.get("reviewers").cloned(),
        _ => None,
    }
}
