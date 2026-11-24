//! `goal.status` (Plan08 03D.4, gate 08G-P03-D2 r2 D2.8): the calling run's goal, read in one
//! repeatable-read snapshot. The run's goal is the one recorded on it; the calling agent must
//! hold `lead` or `sponsor` on that goal now.
use super::*;
use crate::application::ports::department::GOAL_ROLLUP_RUN_LIMIT;
use crate::application::workflow::agent::AgentToolObservation;
use crate::application::workflow::goal_status::{
    GOAL_NONE, GOAL_NOT_AUTHORIZED, GoalStatusDispatch, GoalStatusToolCall,
};
use crate::application::workflow::lease::WorkflowFence;
use crate::application::workflow::tools::tool_error;
use crate::entities::department::DepartmentId;

#[async_trait]
impl GoalStatusDispatch for PostgresPersistence {
    async fn goal_status(
        &self,
        fence: WorkflowFence,
        call: GoalStatusToolCall,
    ) -> AppResult<AgentToolObservation> {
        if !call
            .arguments
            .as_object()
            .is_some_and(|fields| fields.is_empty())
        {
            return Ok(AgentToolObservation::Completed(tool_error(
                "invalid_input",
                "goal.status takes no arguments",
            )));
        }
        let company = fence.scope.company;
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;
        let goal: Option<Uuid> = sqlx::query_scalar(
            "SELECT goal_unit_id FROM workflow_runs WHERE company_id = $1 AND id = $2",
        )
        .bind(company.as_uuid())
        .bind(fence.scope.run.as_uuid())
        .fetch_optional(&mut *tx)
        .await?
        .flatten();
        let Some(goal) = goal else {
            return Ok(AgentToolObservation::Completed(tool_error(
                GOAL_NONE,
                "This run serves no goal",
            )));
        };
        let (is_goal, holds): (bool, bool) = sqlx::query_as(
            "SELECT goal.kind = 'goal', EXISTS ( \
                 SELECT 1 FROM department_roles AS role \
                   JOIN principals AS agent ON agent.company_id = role.company_id \
                    AND agent.id = role.principal_id AND agent.kind = 'agent' AND agent.agent_id = $3 \
                  WHERE role.company_id = $1 AND role.department_id = goal.id \
                    AND role.role IN ('lead', 'sponsor')) \
               FROM departments AS goal WHERE goal.company_id = $1 AND goal.id = $2",
        )
        .bind(company.as_uuid())
        .bind(goal)
        .bind(call.origin.agent.as_uuid())
        .fetch_one(&mut *tx)
        .await?;
        if !is_goal {
            return Ok(AgentToolObservation::Completed(tool_error(
                GOAL_NONE,
                "This run's goal is no longer a goal",
            )));
        }
        if !holds {
            return Ok(AgentToolObservation::Completed(tool_error(
                GOAL_NOT_AUTHORIZED,
                "Only the goal's lead or a sponsor can read its status",
            )));
        }
        let rollup = crate::adapters::persistence::department::goal_rollup::goal_rollup_on(
            &mut tx,
            company,
            DepartmentId::new(goal),
            GOAL_ROLLUP_RUN_LIMIT,
        )
        .await?;
        tx.rollback().await?;
        Ok(AgentToolObservation::Completed(serde_json::json!({
            "status": serde_json::to_value(rollup)
                .map_err(|_| AppError::Internal("Invalid goal status".into()))?
        })))
    }
}
