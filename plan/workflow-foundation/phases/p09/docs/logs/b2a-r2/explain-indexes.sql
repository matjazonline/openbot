-- p09 F029: the run-led indexes the run page needs (also added to the init schema).
CREATE INDEX workflow_control_commands_by_run ON public.workflow_control_commands(company_id, run_id, created_at DESC, command_key DESC);
CREATE INDEX workflow_message_intent_bindings_by_run ON public.workflow_message_intent_bindings(company_id, run_id) WHERE run_id IS NOT NULL;
CREATE INDEX workflow_run_parents_by_parent ON public.workflow_run_parents(company_id, parent_run_id, parent_execution_id);
ANALYZE workflow_control_commands, workflow_message_intent_bindings, workflow_run_parents;
