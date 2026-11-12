//! Account-lock hints for frozen principal-valued final-response policies.
use super::*;
use crate::application::workflow::actions::ActionScope;
use crate::application::workflow::channel_reply::ReplyApprovalPolicy;
use serde_json::Value;

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
pub(super) struct PrincipalAccount {
    pub(super) principal_id: Uuid,
    pub(super) user_id: Uuid,
}
#[derive(PartialEq, Eq)]
pub(super) struct ApprovalAccounts {
    policy: Option<Value>,
    mappings: Vec<PrincipalAccount>,
}
impl ApprovalAccounts {
    pub(super) fn users(&self) -> impl Iterator<Item = Uuid> + '_ {
        self.mappings.iter().map(|mapping| mapping.user_id)
    }
}

/// Discover without principal locks. Call again after account locks and company serialization.
pub(super) async fn load_on(
    db: &mut PgConnection,
    scope: ActionScope,
) -> AppResult<ApprovalAccounts> {
    let policy: Option<Value> = sqlx::query_scalar(
        "SELECT context.approval_policy FROM channel_reply_runs AS owned \
        JOIN channel_reply_contexts AS context USING(company_id,intent_id) \
        WHERE owned.company_id=$1 AND owned.run_id=$2 AND context.approval_policy IS NOT NULL",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_optional(&mut *db)
    .await?;
    let Some(value) = policy.as_ref() else {
        return Ok(ApprovalAccounts {
            policy,
            mappings: Vec::new(),
        });
    };
    let parsed: ReplyApprovalPolicy =
        serde_json::from_value(value.clone()).map_err(|_| invalid())?;
    let mappings = resolve_on(db, scope.company, parsed.reviewers()).await?;
    Ok(ApprovalAccounts { policy, mappings })
}

pub(super) async fn resolve_on(
    db: &mut PgConnection,
    company: CompanyId,
    principals: &[Uuid],
) -> AppResult<Vec<PrincipalAccount>> {
    if principals.is_empty() {
        return Ok(Vec::new());
    }
    if principals.len() > registry::MAX_REVIEWERS {
        return Err(invalid());
    }
    let mappings: Vec<PrincipalAccount> = sqlx::query_as(
        "SELECT id AS principal_id,user_id FROM principals WHERE company_id=$1 AND id=ANY($2) \
         AND kind='person' ORDER BY id LIMIT 17",
    )
    .bind(company.as_uuid())
    .bind(principals)
    .fetch_all(db)
    .await?;
    if mappings.len() != principals.len() {
        return Err(missing());
    }
    Ok(mappings)
}
