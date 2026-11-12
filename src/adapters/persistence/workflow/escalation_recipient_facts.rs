use super::*;
use crate::entities::participant::PrincipalKind;
use crate::entities::transport::PrincipalId;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub(in crate::adapters::persistence::workflow) struct RecipientFact {
    pub principal: PrincipalId,
    pub kind: PrincipalKind,
    pub label: String,
}

pub(in crate::adapters::persistence::workflow) struct HopRecipients {
    pub department: Option<DepartmentId>,
    pub refusal: Option<NotificationRoleRefusal>,
    pub facts: Vec<RecipientFact>,
}

impl HopRecipients {
    pub fn resolution(&self) -> &'static str {
        self.refusal
            .map(NotificationRoleRefusal::as_str)
            .unwrap_or("holders")
    }
    pub fn canonical(&self) -> serde_json::Value {
        serde_json::json!({"department":self.department,"resolution":self.resolution(),"recipients":self.facts})
    }
}

#[derive(sqlx::FromRow)]
struct PrincipalRow {
    id: Uuid,
    kind: String,
    label: String,
}

pub(in crate::adapters::persistence::workflow) async fn facts_on(
    db: &mut PgConnection,
    company: CompanyId,
    selection: &NotificationRecipients,
) -> AppResult<HopRecipients> {
    let (department, holders) = match selection {
        NotificationRecipients::Refused { department, reason } => {
            return Ok(HopRecipients {
                department: *department,
                refusal: Some(*reason),
                facts: Vec::new(),
            });
        }
        NotificationRecipients::Holders {
            department,
            holders,
        } => (*department, holders),
    };
    let identities: Vec<_> = holders
        .iter()
        .map(|holder| holder.principal.as_uuid())
        .collect();
    let rows = sqlx::query_as::<_, PrincipalRow>(
        "SELECT id,kind,display_label AS label FROM principals WHERE company_id=$1 AND id=ANY($2) ORDER BY id",
    )
    .bind(company.as_uuid())
    .bind(&identities)
    .fetch_all(db)
    .await?;
    if rows.len() != holders.len()
        || rows.iter().zip(holders).any(|(row, holder)| {
            row.id != holder.principal.as_uuid() || row.kind != holder.kind.as_str()
        })
    {
        return Err(crate::application::workflow::escalation::invalid());
    }
    let facts = rows
        .into_iter()
        .zip(holders)
        .map(|(row, holder)| RecipientFact {
            principal: holder.principal,
            kind: holder.kind,
            label: row.label,
        })
        .collect();
    Ok(HopRecipients {
        department: Some(department),
        refusal: None,
        facts,
    })
}
