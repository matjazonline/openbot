CREATE FUNCTION public.native_mcp_run_live(tenant uuid, owner uuid) RETURNS boolean LANGUAGE sql STABLE AS $$
    SELECT CASE WHEN run.native_mcp_call_id IS NULL THEN
        NOT EXISTS(SELECT 1 FROM public.native_mcp_brokers WHERE company_id=tenant AND version_id=run.version_id)
    ELSE EXISTS(SELECT 1 FROM public.native_mcp_calls AS call
        JOIN public.native_mcp_brokers AS broker ON broker.company_id=call.company_id AND broker.id=call.broker_id
        JOIN public.local_agent_operations AS operation ON operation.company_id=call.company_id AND operation.id=call.operation_id
        JOIN public.local_agent_sessions AS session ON session.company_id=call.company_id AND session.id=call.session_id
        JOIN public.local_machines AS machine ON machine.company_id=call.company_id AND machine.id=call.machine_id
        JOIN public.local_machine_credentials AS credential ON credential.company_id=machine.company_id
            AND credential.machine_id=machine.id AND credential.generation=machine.credential_generation
        JOIN public.company_members AS member ON member.company_id=call.company_id AND member.user_id=call.actor_id
        JOIN public.principals AS principal ON principal.company_id=member.company_id AND principal.user_id=member.user_id
        JOIN public.users AS account ON account.id=member.user_id
        WHERE call.company_id=tenant AND call.id=run.native_mcp_call_id AND call.run_id=owner AND call.retired_at IS NULL
            AND NOT call.cancel_requested AND call.deadline>clock_timestamp()
            AND operation.actor_id=call.actor_id AND operation.outcome IS NULL AND operation.deadline>clock_timestamp()
            AND NOT public.native_mcp_stop_requested(tenant,operation.id)
            AND operation.dispatch->>'kind'='native_identity' AND operation.dispatch->>'thread_id'=call.native_thread_id
            AND operation.dispatch->>'turn_id'=call.native_turn_id
            AND (operation.observed_turn_state IS NULL OR operation.observed_turn_state IN ('queued','running','waiting_for_input'))
            AND machine.state='enabled' AND machine.connection_owner IS NOT NULL AND machine.connection_expires_at>clock_timestamp()
            AND credential.revoked_at IS NULL AND session.agent_revision_id=broker.agent_revision_id
            AND session.bundle_digest=call.harness_digest AND session.thread_id=call.native_thread_id
            AND principal.kind='person' AND member.role IN ('owner','admin')
            AND public.native_mcp_revision_live(tenant,broker.agent_revision_id)
            AND EXISTS(SELECT 1 FROM public.local_machine_projects WHERE company_id=tenant AND machine_id=call.machine_id
                AND project_id=session.selection->>'project_id')
            AND (session.workflow_run_id IS NULL OR EXISTS(SELECT 1 FROM public.workflow_runs AS original
                WHERE original.company_id=tenant AND original.id=session.workflow_run_id
                    AND original.state IN ('queued','running','waiting') AND original.deadline>clock_timestamp()))
            AND (operation.origin='direct' OR EXISTS(SELECT 1 FROM public.workflow_local_agent_calls AS original_call
                JOIN public.workflow_runs AS original ON original.company_id=original_call.company_id AND original.id=original_call.run_id
                JOIN public.workflow_waits AS wait ON wait.company_id=original_call.company_id AND wait.id=original_call.wait_id
                JOIN public.workflow_action_intents AS intent ON intent.company_id=original_call.company_id AND intent.id=original_call.invocation_id
                WHERE original_call.company_id=tenant AND original_call.operation_id=operation.id
                    AND original.state='waiting' AND original.waiting_reason='effect' AND original.deadline>clock_timestamp()
                    AND original_call.ready_at IS NULL AND wait.state='waiting' AND wait.deadline>clock_timestamp()
                    AND (intent.origin_kind='workflow' OR (intent.origin_kind='agent'
                        AND public.native_mcp_parent_agent_live(tenant,intent.agent_revision_id)))))
    ) END FROM public.workflow_runs AS run WHERE run.company_id=tenant AND run.id=owner
$$;
