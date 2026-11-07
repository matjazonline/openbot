WITH tree AS MATERIALIZED (
    SELECT run_id FROM workflow_run_budgets WHERE company_id=$1 AND root_run_id=$2
)
SELECT
    NOT EXISTS(SELECT 1 FROM workflow_runs AS run JOIN tree ON tree.run_id=run.id
        WHERE run.company_id=$1 AND run.state NOT IN ('succeeded','failed','cancelled'))
    AND NOT EXISTS(SELECT 1 FROM background_tasks AS job JOIN workflow_executions AS execution
        ON execution.company_id=job.company_id AND execution.id=job.workflow_execution_id
        JOIN tree ON tree.run_id=execution.run_id WHERE execution.company_id=$1 AND job.status IN ('pending','processing'))
    AND NOT EXISTS(SELECT 1 FROM task_attempts AS attempt JOIN background_tasks AS job ON job.id=attempt.task_id
        JOIN workflow_executions AS execution ON execution.company_id=job.company_id AND execution.id=job.workflow_execution_id
        JOIN tree ON tree.run_id=execution.run_id WHERE execution.company_id=$1 AND attempt.status='processing')
    AND NOT EXISTS(SELECT 1 FROM workflow_decisions AS decision JOIN workflow_waits AS wait
        ON wait.company_id=decision.company_id AND wait.run_id=decision.run_id AND wait.id=decision.wait_id
        JOIN tree ON tree.run_id=decision.run_id WHERE decision.company_id=$1 AND decision.submitted_at IS NULL AND wait.state='waiting')
    AND NOT EXISTS(SELECT 1 FROM workflow_message_actions AS action JOIN tree ON tree.run_id=action.run_id
        WHERE action.company_id=$1 AND action.state IN ('pending','uncertain','stalled'))
    AND NOT EXISTS(SELECT 1 FROM channel_reply_decisions AS final JOIN tree ON tree.run_id=final.run_id
        LEFT JOIN message_deliveries AS delivery ON delivery.company_id=final.company_id AND delivery.id=final.delivery_id
        WHERE final.company_id=$1 AND (final.state IN ('pending','claimed')
            OR (final.state='published' AND final.delivery_id IS NOT NULL
                AND (delivery.id IS NULL OR delivery.status NOT IN ('delivered','dead_letter')))))
    AND NOT EXISTS(SELECT 1 FROM workflow_runs AS run JOIN tree ON tree.run_id=run.rerun_of_run_id
        WHERE run.company_id=$1 AND NOT EXISTS(SELECT 1 FROM tree AS member WHERE member.run_id=run.id))
    AND NOT EXISTS(SELECT 1 FROM workflow_action_evidence_conflicts AS conflict JOIN tree ON tree.run_id=conflict.run_id WHERE conflict.company_id=$1)
    AND NOT EXISTS(SELECT 1 FROM workflow_action_dispatches AS marker JOIN tree ON tree.run_id=marker.run_id WHERE marker.company_id=$1
        AND NOT EXISTS(SELECT 1 FROM workflow_action_receipts WHERE company_id=marker.company_id AND invocation_id=marker.invocation_id)
        AND NOT EXISTS(SELECT 1 FROM workflow_action_evidence WHERE company_id=marker.company_id AND invocation_id=marker.invocation_id AND disposition='applied')
        AND NOT (marker.effect_kind='remote' AND EXISTS(
            SELECT 1 FROM workflow_action_evidence AS proof WHERE proof.company_id=marker.company_id AND proof.invocation_id=marker.invocation_id
                AND proof.disposition='final_not_applied' AND proof.grant_eligible
                -- Historical closure survives grant expiry, but cannot cover a later entry.
                AND NOT EXISTS(SELECT 1 FROM workflow_action_remote_entries AS entry
                    WHERE entry.company_id=marker.company_id AND entry.invocation_id=marker.invocation_id
                    AND NOT EXISTS(SELECT 1 FROM workflow_action_evidence_coverage AS coverage
                        WHERE coverage.company_id=proof.company_id AND coverage.evidence_id=proof.id AND coverage.remote_entry_id=entry.id)))))
