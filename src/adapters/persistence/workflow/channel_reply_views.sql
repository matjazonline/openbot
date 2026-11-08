-- Only the actual admitted owner grants a response identity; passive snapshots are excluded.
SELECT run.id AS run_id, run.state, review.run_id AS review_run,
       context.message_id AS source_message, context.channel_id, context.thread_id,
       decision.state AS publication_state, decision.message_id AS final_message,
       decision.delivery_id AS final_delivery, delivery.status AS final_delivery_state,
       prompt.run_id AS prompt_run, prompt.wait_id AS prompt_wait, prompt.message_id AS prompt_message,
       prompt.deadline AS prompt_deadline, prompt.waiting AS prompt_waiting,
       prompt.awaiting_viewer, prompt.delivery_id AS prompt_delivery,
       prompt.delivery_state AS prompt_delivery_state
FROM workflow_runs AS run
JOIN channel_reply_runs AS owner ON owner.company_id=run.company_id AND owner.run_id=run.id
JOIN channel_reply_contexts AS context ON context.company_id=owner.company_id AND context.intent_id=owner.intent_id
JOIN workflow_run_budgets AS budget ON budget.company_id=run.company_id AND budget.run_id=run.id
LEFT JOIN channel_reply_decisions AS decision ON decision.company_id=run.company_id AND decision.run_id=run.id
LEFT JOIN message_deliveries AS delivery ON delivery.company_id=decision.company_id AND delivery.id=decision.delivery_id
LEFT JOIN LATERAL (
    SELECT candidate.id AS run_id
    FROM workflow_run_budgets AS review_budget
    JOIN workflow_runs AS candidate ON candidate.company_id=review_budget.company_id
        AND candidate.id=review_budget.run_id
    WHERE review_budget.company_id=run.company_id AND review_budget.root_run_id=budget.root_run_id
        AND candidate.channel_id=context.channel_id AND candidate.thread_id=context.thread_id
        AND run.state='waiting' AND candidate.state='waiting' AND candidate.waiting_reason='decision'
    ORDER BY candidate.created_at DESC,candidate.id DESC LIMIT 1
) AS review ON true
LEFT JOIN LATERAL (
    SELECT question.run_id, question.wait_id, question.message_id, question.deadline,
           wait.state='waiting' AND question.deadline>clock_timestamp() AS waiting,
           EXISTS(SELECT 1 FROM principals AS principal WHERE principal.company_id=question.company_id
               AND principal.id=question.respondent_id AND principal.user_id=$3) AS awaiting_viewer,
           destination.delivery_id, prompt_delivery.status AS delivery_state
    FROM workflow_channel_questions AS question
    JOIN workflow_run_budgets AS child_budget ON child_budget.company_id=question.company_id
        AND child_budget.run_id=question.run_id AND child_budget.root_run_id=budget.root_run_id
    JOIN workflow_waits AS wait ON wait.company_id=question.company_id AND wait.id=question.wait_id
    JOIN workflow_message_destinations AS destination ON destination.company_id=question.company_id
        AND destination.invocation_id=question.invocation_id AND destination.message_id=question.message_id
        AND destination.channel_id=context.channel_id AND destination.thread_id=context.thread_id
    LEFT JOIN message_deliveries AS prompt_delivery ON prompt_delivery.company_id=destination.company_id
        AND prompt_delivery.id=destination.delivery_id
    -- Later questions may target a previous human answer. Ownership and the exact
    -- destination above retain the original admitted response without fixing every turn to M0.
    WHERE question.company_id=run.company_id
    ORDER BY (wait.state='waiting' AND question.deadline>clock_timestamp()) DESC,
        wait.created_at DESC, question.wait_id DESC
    LIMIT 1
) AS prompt ON true
WHERE run.company_id=$1 AND run.id=ANY($2)
ORDER BY run.created_at DESC, run.id DESC
LIMIT 50
