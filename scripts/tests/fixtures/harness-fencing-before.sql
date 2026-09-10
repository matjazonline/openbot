-- Historical query-plan comparison fixture, captured before the 2026-09-10 fencing optimization.
-- Never apply this file as a migration; harness-query-plans.py extracts SELECTs only.

CREATE FUNCTION public.lock_task_agent_harnesses() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.status IN ('pending', 'processing', 'pending_approval',
                      'waiting_for_third_party_reply', 'stopped', 'failed', 'dead_letter') THEN
        PERFORM agent.id FROM agents AS agent
        WHERE EXISTS (SELECT 1 FROM principals AS principal
                      WHERE principal.id = NEW.owner_principal_id
                        AND principal.company_id = NEW.company_id
                        AND principal.agent_id = agent.id)
           OR EXISTS (SELECT 1 FROM channel_agents AS assignment
                      WHERE assignment.channel_id = NEW.channel_id
                        AND assignment.company_id = NEW.company_id
                        AND assignment.agent_id = agent.id)
        ORDER BY agent.id FOR SHARE OF agent;
    END IF;
    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION public.guard_active_agent_harness_change() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.harness_kind IS NOT DISTINCT FROM OLD.harness_kind
       AND NEW.response_contract IS NOT DISTINCT FROM OLD.response_contract THEN
        RETURN NEW;
    END IF;
    IF EXISTS (
        SELECT 1 FROM background_tasks AS task
        WHERE task.status IN ('pending', 'processing', 'pending_approval',
                              'waiting_for_third_party_reply', 'stopped', 'failed', 'dead_letter')
          AND (
              EXISTS (SELECT 1 FROM principals AS principal
                      WHERE principal.id = task.owner_principal_id
                        AND principal.company_id = task.company_id
                        AND principal.agent_id = OLD.id)
              OR EXISTS (SELECT 1 FROM channel_agents AS assignment
                         WHERE assignment.channel_id = task.channel_id
                           AND assignment.company_id = task.company_id
                           AND assignment.agent_id = OLD.id)
          )
    ) THEN
        RAISE EXCEPTION USING ERRCODE = '23514',
            CONSTRAINT = 'agent_harness_has_unsettled_tasks',
            MESSAGE = 'Agent harness cannot change while tasks are active or suspended';
    END IF;
    RETURN NEW;
END;
$$;
