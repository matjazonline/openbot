use super::super::persistence_bound::{Evidence, WriteBound};
use super::*;

pub(super) async fn resolve_on(
    runtime: &PostgresLocalAgentWorkflow,
    dispatch: &Dispatch,
    claim: &ClaimedWorkflow,
    policy: LeasePolicy,
    bound: &WriteBound,
) -> AppResult<Evidence<contract::LocalRunProgress>> {
    let mut connection = bound.acquire(runtime.persistence.pool()).await?;
    let mut tx = bound.begin(&mut connection).await?;
    let proof = async {
        let actor = action_authority::prelock_on(&mut tx, action_scope(claim.fence.scope)).await?;
        lease::lock_scope(&mut tx, claim.fence.scope, bound.server_timeout()).await?;
        let Some(call) = saved_on(&mut tx, action_scope(claim.fence.scope)).await? else {
            return Ok(Evidence::NotApplied);
        };
        let request = ActionDispatchRequest {
            subject: call.subject()?,
            fence: claim.fence,
            lease: policy,
        };
        if let Some(wait) = decisions::approvals::parked(&mut tx, &request).await? {
            return Ok(Evidence::Applied(contract::LocalRunProgress::Suspended(
                wait,
            )));
        }
        if let Some(wait) = call.wait_id
            && continuation::confirm_on(&mut tx, claim.fence, wait).await?
        {
            return Ok(Evidence::Applied(contract::LocalRunProgress::Suspended(
                WaitId::new(wait),
            )));
        }
        let action = call.action()?;
        // A saved receipt proves COMMIT. Return it only under the current claim and live
        // action/resource authority; a historical marker alone never authorizes its output.
        let saved = continuation::saved_result_on(&mut tx, &request, &action).await?;
        if let Some(saved) = saved {
            let authorized = dispatch
                .authorize_prelocked_on(&mut tx, &request, bound.server_timeout(), actor)
                .await?;
            if authorized.approval_required {
                return Err(conflict());
            }
            return Ok(Evidence::Applied(saved));
        }
        Ok(Evidence::NotApplied)
    }
    .await;
    bound.roll_back(tx, proof).await
}
