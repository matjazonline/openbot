from pathlib import Path
p=Path('migrations/20260817000000_init_schema.sql');s=p.read_text()
def replace(old,new,count=1):
 global s
 assert s.count(old)==count,(old,s.count(old),count)
 s=s.replace(old,new)
replace('    CONSTRAINT background_tasks_retry_count_check CHECK ((retry_count >= 0)),','    CONSTRAINT background_tasks_retry_count_check CHECK (retry_count BETWEEN 0 AND max_retries),')
old=next(line for line in s.splitlines() if 'CONSTRAINT task_attempts_stop_reason_check' in line)
replace(old,'''    CONSTRAINT task_attempts_finished_check CHECK ((status = 'processing') = (finished_at IS NULL)),
    CONSTRAINT task_attempts_stop_reason_check CHECK (CASE status
        WHEN 'processing' THEN stop_reason IS NULL
        WHEN 'completed' THEN stop_reason IS NOT NULL AND stop_reason IN ('completed','effect_parked','approval_parked','child_parked')
        WHEN 'failed' THEN stop_reason IS NULL OR stop_reason IN ('timed_out','lease_lost')
        ELSE false END),''')
anchor='''--
-- Name: task_harness_runs; Type: TABLE; Schema: public; Owner: -
--'''
replace(anchor,'''-- A job has at most one active lease episode, even if attempt numbers differ.
CREATE UNIQUE INDEX task_attempts_one_processing ON public.task_attempts(task_id) WHERE status = 'processing';


'''+anchor)
old=next(line for line in s.splitlines() if 'CONSTRAINT message_deliveries_error_check' in line)
replace(old,'''    CONSTRAINT message_deliveries_error_check CHECK (
        (last_error_class IS NULL OR public.valid_delivery_failure_class(last_error_class))
        AND (last_error_detail IS NULL OR (octet_length(last_error_detail) <= 512 AND last_error_class IS NOT NULL))
        AND (status NOT IN ('retryable','dead_letter') OR last_error_class IS NOT NULL)),''')
old=next(line for line in s.splitlines() if 'CONSTRAINT notification_events_error_check' in line)
replace(old,'''    CONSTRAINT notification_events_error_check CHECK (
        (last_error_class IS NULL OR last_error_class IN ('database','composition','invalid_source','lease_expired','internal','stale_projection'))
        AND (last_error_detail IS NULL OR (octet_length(last_error_detail) <= 512 AND last_error_class IS NOT NULL))
        AND (status <> 'dead_letter' OR last_error_class IS NOT NULL)),''')
replace('    CONSTRAINT agents_name_not_blank CHECK ((btrim(name) <> \'\'::text)),', '''    CONSTRAINT agents_model_pair_check CHECK ((provider IS NULL) = (model IS NULL)),
    CONSTRAINT agents_provider_check CHECK (provider IS NULL OR provider IN ('google','openai','anthropic','groq')),
    CONSTRAINT agents_name_not_blank CHECK ((btrim(name) <> ''::text)),''')
replace("    CONSTRAINT company_mcp_connections_auth_type_check CHECK ((auth_type = ANY (ARRAY['none'::text, 'bearer'::text]))),", "    CONSTRAINT company_mcp_connections_auth_type_check CHECK ((auth_type = ANY (ARRAY['none'::text, 'bearer'::text]))),\n    CONSTRAINT company_mcp_connections_deleted_check CHECK (deleted_at IS NULL OR NOT enabled),")
replace('''ALTER TABLE ONLY public.company_mcp_connections
    ADD CONSTRAINT company_mcp_connections_company_id_slug_key UNIQUE (company_id, slug);''','''CREATE UNIQUE INDEX company_mcp_connections_company_id_slug_key
    ON public.company_mcp_connections(company_id, slug) WHERE deleted_at IS NULL;''')
replace('-- Name: company_mcp_connections company_mcp_connections_company_id_slug_key; Type: CONSTRAINT;', '-- Name: company_mcp_connections_company_id_slug_key; Type: INDEX;')
p.write_text(s)
p=Path('src/adapters/persistence/notification.rs');s=p.read_text();a=s.index('async fn release_stale_projection(');b=s.index('\n}',a);part=s[a:b];old='''                  execution_id = NULL, owner_worker_id = NULL,''';new='''                  last_error_class = 'stale_projection',
                  last_error_detail = 'Notification projection changed before commit',
                  execution_id = NULL, owner_worker_id = NULL,''';assert part.count(old)==1;part=part.replace(old,new);p.write_text(s[:a]+part+s[b:])
p=Path('src/adapters/persistence/workflow/activation_tests.rs');s=p.read_text();old="INSERT INTO task_attempts (id, task_id, attempt_number, status, execution_generation, worker_id) VALUES (gen_random_uuid(), $1, $2, 'failed', gen_random_uuid(), gen_random_uuid())";new="INSERT INTO task_attempts (id, task_id, attempt_number, status, execution_generation, worker_id, finished_at) VALUES (gen_random_uuid(), $1, $2, 'failed', gen_random_uuid(), gen_random_uuid(), clock_timestamp())";assert s.count(old)==1;p.write_text(s.replace(old,new))
p=Path('src/adapters/persistence/workflow/job_projection_tests.rs');s=p.read_text();old="INSERT INTO task_attempts (id,task_id,attempt_number,execution_generation,status,worker_id,started_at,finished_at) VALUES (gen_random_uuid(),$1,2,gen_random_uuid(),'completed',gen_random_uuid(),CURRENT_TIMESTAMP";new="INSERT INTO task_attempts (id,task_id,attempt_number,execution_generation,status,stop_reason,worker_id,started_at,finished_at) VALUES (gen_random_uuid(),$1,2,gen_random_uuid(),'completed','completed',gen_random_uuid(),CURRENT_TIMESTAMP";assert s.count(old)==1;p.write_text(s.replace(old,new))
print('Missing constraints and stale-class writer applied; queue constants retained until last step')
