from pathlib import Path
p=Path('migrations/20260817000000_init_schema.sql'); s=p.read_text()
s=s.replace("'effect_parked'::text])))", "'effect_parked'::text, 'approval_parked'::text])))")
s=s.replace('ADD CONSTRAINT workflow_wait_execution UNIQUE (company_id, run_id, execution_id);','ADD CONSTRAINT workflow_wait_execution UNIQUE (company_id, run_id, execution_id, reason);\n\nCREATE UNIQUE INDEX workflow_wait_one_active ON public.workflow_waits(company_id,run_id,execution_id) WHERE state=\'waiting\';')
s=s.replace('    subject_kind text NOT NULL,\n    proposal_digest text NOT NULL,', '    subject_kind text NOT NULL,\n    action_invocation_id uuid,\n    argument_digest text,\n    operation_digest text,\n    submitted_xact xid8,\n    proposal_digest text NOT NULL,')
s=s.replace("CONSTRAINT workflow_decisions_subject_kind_check CHECK ((subject_kind = 'business'::text))", "CONSTRAINT workflow_decisions_subject_kind_check CHECK ((subject_kind='business' AND action_invocation_id IS NULL AND argument_digest IS NULL AND operation_digest IS NULL) OR (subject_kind='action_approval' AND action_invocation_id IS NOT NULL AND argument_digest IS NOT NULL AND operation_digest IS NOT NULL)),\n    CONSTRAINT workflow_decisions_submission_xact CHECK ((submitted_at IS NULL) = (submitted_xact IS NULL))")
s=s.replace('NEW.proposal_digest, NEW.created_at)\n            IS NOT DISTINCT FROM (OLD.company_id', 'NEW.proposal_digest, NEW.created_at, NEW.action_invocation_id, NEW.argument_digest, NEW.operation_digest)\n            IS NOT DISTINCT FROM (OLD.company_id')
s=s.replace('OLD.subject_kind, OLD.proposal_digest, OLD.created_at) THEN', 'OLD.subject_kind, OLD.proposal_digest, OLD.created_at, OLD.action_invocation_id, OLD.argument_digest, OLD.operation_digest) THEN')
s=s.replace('        PERFORM workflow_decision_person_snapshot(NEW.company_id, NEW.submitted_by);', '        PERFORM workflow_decision_person_snapshot(NEW.company_id, NEW.submitted_by);\n        NEW.submitted_xact := pg_current_xact_id();')
# Existing deferred completion guard additionally owns both approval park and reopen.
a=s.index('CREATE FUNCTION public.check_workflow_fenced_completion()'); b=s.index('END $$;',a)
part=s[a:b]
old="AND wait.reason = 'effect' AND wait.state = 'waiting')))"
new="""AND wait.reason = 'effect' AND wait.state = 'waiting'))
                        OR (attempt.stop_reason='approval_parked' AND execution.completed_at IS NULL
                            AND execution.committed_route IS NULL AND run.state='waiting' AND run.waiting_reason='decision'
                            AND OLD.retry_count + 1 < OLD.max_retries
                            AND EXISTS (SELECT 1 FROM workflow_decisions AS decision
                                JOIN workflow_waits AS wait ON wait.company_id=decision.company_id AND wait.id=decision.wait_id
                                WHERE decision.company_id=execution.company_id AND decision.execution_id=execution.id
                                    AND decision.subject_kind='action_approval' AND decision.submitted_at IS NULL
                                    AND wait.reason='decision' AND wait.state='waiting')))"""
assert old in part; part=part.replace(old,new)
part=part.replace('    RETURN NULL;', """    IF OLD.queue_kind='workflow' AND OLD.status='completed' AND NEW.status='pending' THEN
        IF NEW.retry_count <> OLD.retry_count + 1 OR NEW.max_retries <> OLD.max_retries
            OR NEW.retry_count >= NEW.max_retries
            OR (NEW.company_id,NEW.workflow_execution_id) IS DISTINCT FROM (OLD.company_id,OLD.workflow_execution_id)
            OR NEW.worker_id IS NOT NULL OR NEW.execution_generation IS NOT NULL OR NEW.lock_expires_at IS NOT NULL
            OR NOT EXISTS (
                SELECT 1 FROM workflow_executions AS execution
                JOIN workflow_runs AS run ON run.company_id=execution.company_id AND run.id=execution.run_id
                JOIN workflow_decisions AS decision ON decision.company_id=execution.company_id AND decision.execution_id=execution.id
                JOIN workflow_waits AS wait ON wait.company_id=decision.company_id AND wait.id=decision.wait_id
                JOIN task_attempts AS attempt ON attempt.task_id=OLD.id AND attempt.attempt_number=OLD.retry_count+1
                WHERE execution.company_id=NEW.company_id AND execution.id=NEW.workflow_execution_id
                    AND execution.completed_at IS NULL AND execution.committed_route IS NULL
                    AND run.state='running' AND run.deadline>clock_timestamp()
                    AND attempt.status='completed' AND attempt.stop_reason='approval_parked'
                    AND decision.subject_kind='action_approval' AND decision.choice='approve'
                    AND decision.submitted_xact=pg_current_xact_id()
                    AND wait.reason='decision' AND wait.state='completed'
            ) THEN
            RAISE EXCEPTION 'workflow job reopen requires an approval in this transaction' USING ERRCODE='23514';
        END IF;
    END IF;
    RETURN NULL;""")
s=s[:a]+part+s[b:]
s += """
-- Approval names one immutable operation, including both digests (05-D2 §16.3).
ALTER TABLE public.workflow_action_intents ADD CONSTRAINT workflow_action_approval_identity
    UNIQUE(company_id,run_id,execution_id,id,argument_digest,operation_digest);
ALTER TABLE public.workflow_decisions ADD CONSTRAINT workflow_decisions_action_identity
    FOREIGN KEY(company_id,run_id,execution_id,action_invocation_id,argument_digest,operation_digest)
    REFERENCES public.workflow_action_intents(company_id,run_id,execution_id,id,argument_digest,operation_digest);
"""
p.write_text(s)
