# D2/r1 independently inspected ownership evidence

Frozen source 26c71787f442752d2de436e197dcb0da8b36c5eaeca73103d925508b65c31a62. Static source extraction only; no DDL/runtime probe executed.

## threads
- init schema:10221: `ALTER TABLE ONLY public.threads ADD CONSTRAINT threads_channel_fk FOREIGN KEY (company_id, channel_id) REFERENCES public.channels(company_id, id) ON DELETE CASCADE;`
- init schema:18971: `ALTER TABLE public.threads ADD CONSTRAINT threads_target_principal_fk FOREIGN KEY (company_id, principal_id) REFERENCES public.principals(company_id, id);`

## thread_messages
- init schema:10189: `ALTER TABLE ONLY public.thread_messages ADD CONSTRAINT thread_messages_message_fk FOREIGN KEY (company_id, message_id) REFERENCES public.messages(company_id, id) ON DELETE CASCADE;`
- init schema:10197: `ALTER TABLE ONLY public.thread_messages ADD CONSTRAINT thread_messages_thread_fk FOREIGN KEY (company_id, channel_id, thread_id) REFERENCES public.threads(company_id, channel_id, id) ON DELETE CASCADE;`
- init schema:18967: `ALTER TABLE public.thread_messages ADD CONSTRAINT thread_messages_thread_fk_principal_fk FOREIGN KEY (company_id, principal_id, thread_id) REFERENCES public.threads(company_id, principal_id, id) ON DELETE CASCADE;`
- init schema:18968: `ALTER TABLE public.thread_messages ADD CONSTRAINT thread_messages_target_principal_fk FOREIGN KEY (company_id, principal_id) REFERENCES public.principals(company_id, id);`

## thread_principals
- init schema:10205: `ALTER TABLE ONLY public.thread_principals ADD CONSTRAINT thread_principals_principal_fk FOREIGN KEY (company_id, principal_id) REFERENCES public.principals(company_id, id) ON DELETE CASCADE;`
- init schema:10213: `ALTER TABLE ONLY public.thread_principals ADD CONSTRAINT thread_principals_thread_fk FOREIGN KEY (company_id, channel_id, thread_id) REFERENCES public.threads(company_id, channel_id, id) ON DELETE CASCADE;`
- init schema:18969: `ALTER TABLE public.thread_principals ADD CONSTRAINT thread_principals_thread_fk_principal_fk FOREIGN KEY (company_id, target_principal_id, thread_id) REFERENCES public.threads(company_id, principal_id, id) ON DELETE CASCADE;`
- init schema:18970: `ALTER TABLE public.thread_principals ADD CONSTRAINT thread_principals_target_principal_fk FOREIGN KEY (company_id, target_principal_id) REFERENCES public.principals(company_id, id);`

## workflow_message_intents
- init schema:11035: `ALTER TABLE ONLY public.workflow_message_intents ADD CONSTRAINT workflow_message_intent_company FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;`
- init schema:11043: `ALTER TABLE ONLY public.workflow_message_intents ADD CONSTRAINT workflow_message_intent_association FOREIGN KEY (company_id, channel_id, thread_id, message_id) REFERENCES public.thread_messages(company_id, channel_id, thread_id, message_id) ON DELETE CASCADE;`
- init schema:11067: `ALTER TABLE ONLY public.workflow_message_intents ADD CONSTRAINT workflow_message_intent_matched_run FOREIGN KEY (company_id, matched_run_id) REFERENCES public.workflow_runs(company_id, id) ON DELETE CASCADE;`
- init schema:18982: `ALTER TABLE public.workflow_message_intents ADD CONSTRAINT workflow_message_intent_association_principal_fk FOREIGN KEY (company_id, principal_id, thread_id, message_id) REFERENCES public.thread_messages(company_id, principal_id, thread_id, message_id) ON DELETE CASCADE;`
- init schema:18983: `ALTER TABLE public.workflow_message_intents ADD CONSTRAINT workflow_message_intents_target_principal_fk FOREIGN KEY (company_id, principal_id) REFERENCES public.principals(company_id, id);`

## workflow_message_destinations
- init schema:10979: `ALTER TABLE ONLY public.workflow_message_destinations ADD CONSTRAINT workflow_message_destination_action FOREIGN KEY (company_id, invocation_id) REFERENCES public.workflow_message_actions(company_id, invocation_id) ON DELETE CASCADE;`
- init schema:11003: `ALTER TABLE ONLY public.workflow_message_destinations ADD CONSTRAINT workflow_message_destination_owner FOREIGN KEY (company_id, run_id) REFERENCES public.workflow_runs(company_id, id) ON DELETE CASCADE;`
- init schema:18981: `ALTER TABLE public.workflow_message_destinations ADD CONSTRAINT workflow_message_destinations_target_principal_fk FOREIGN KEY (company_id, target_principal_id) REFERENCES public.principals(company_id, id);`

## workflow_history_members
- init schema:10890: `ALTER TABLE ONLY public.workflow_history_members ADD CONSTRAINT workflow_history_members_company_id_run_id_channel_id_thre_fkey FOREIGN KEY (company_id, run_id, channel_id, thread_id) REFERENCES public.workflow_runs(company_id, id, channel_id, thread_id) ON DELETE CASCADE;`
- init schema:18977: `ALTER TABLE public.workflow_history_members ADD CONSTRAINT workflow_history_members_company_id_run_id_c_principal_fk FOREIGN KEY (company_id, run_id, principal_id, thread_id) REFERENCES public.workflow_runs(company_id, id, principal_id, thread_id) ON DELETE CASCADE;`
- init schema:18978: `ALTER TABLE public.workflow_history_members ADD CONSTRAINT workflow_history_members_target_principal_fk FOREIGN KEY (company_id, principal_id) REFERENCES public.principals(company_id, id);`

## workflow_runs
- init schema:11288: `ALTER TABLE ONLY public.workflow_runs ADD CONSTRAINT workflow_runs_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;`
- init schema:18985: `ALTER TABLE public.workflow_runs ADD CONSTRAINT workflow_runs_target_principal_fk FOREIGN KEY (company_id, principal_id) REFERENCES public.principals(company_id, id);`

## workflow_admission_refusals
- init schema:18973: `ALTER TABLE public.workflow_admission_refusals ADD CONSTRAINT workflow_admission_refusals_target_principal_fk FOREIGN KEY (company_id, principal_id) REFERENCES public.principals(company_id, id);`
- init schema:16992: `company_id uuid NOT NULL REFERENCES public.companies(id) ON DELETE CASCADE,`

## agent_history_sessions
- init schema:18950: `ALTER TABLE public.agent_history_sessions ADD CONSTRAINT agent_history_sessions_target_principal_fk FOREIGN KEY (company_id, principal_id) REFERENCES public.principals(company_id, id);`
- init schema:14261: `company_id uuid NOT NULL REFERENCES public.companies(id) ON DELETE CASCADE,`

## agent_history_conversations
- init schema:18948: `ALTER TABLE public.agent_history_conversations ADD CONSTRAINT agent_history_conversations_target_principal_fk FOREIGN KEY (company_id, principal_id) REFERENCES public.principals(company_id, id);`
- init schema:14291: `company_id uuid NOT NULL REFERENCES public.companies(id) ON DELETE CASCADE,`

## background_tasks
- init schema:9533: `ALTER TABLE ONLY public.background_tasks ADD CONSTRAINT background_tasks_channel_fk FOREIGN KEY (company_id, channel_id) REFERENCES public.channels(company_id, id) ON DELETE CASCADE;`
- init schema:9541: `ALTER TABLE ONLY public.background_tasks ADD CONSTRAINT background_tasks_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;`
- init schema:18952: `ALTER TABLE public.background_tasks ADD CONSTRAINT background_tasks_target_principal_fk FOREIGN KEY (company_id, principal_id) REFERENCES public.principals(company_id, id);`

## channel_reply_contexts
- init schema:18954: `ALTER TABLE public.channel_reply_contexts ADD CONSTRAINT channel_reply_contexts_target_principal_fk FOREIGN KEY (company_id, principal_id) REFERENCES public.principals(company_id, id);`
- init schema:12866: `company_id uuid NOT NULL REFERENCES public.companies(id) ON DELETE CASCADE,`

## external_threads
- init schema:9766: `ALTER TABLE ONLY public.external_threads ADD CONSTRAINT external_threads_binding_fk FOREIGN KEY (company_id, channel_id, binding_id) REFERENCES public.channel_bindings(company_id, channel_id, id) ON DELETE CASCADE;`
- init schema:9774: `ALTER TABLE ONLY public.external_threads ADD CONSTRAINT external_threads_thread_fk FOREIGN KEY (company_id, channel_id, thread_id) REFERENCES public.threads(company_id, channel_id, id) ON DELETE CASCADE;`
- init schema:18957: `ALTER TABLE public.external_threads ADD CONSTRAINT external_threads_binding_fk_principal_fk FOREIGN KEY (company_id, principal_id, binding_id) REFERENCES public.channel_bindings(company_id, principal_id, id) ON DELETE CASCADE;`
- init schema:18958: `ALTER TABLE public.external_threads ADD CONSTRAINT external_threads_thread_fk_principal_fk FOREIGN KEY (company_id, principal_id, thread_id) REFERENCES public.threads(company_id, principal_id, id) ON DELETE CASCADE;`
- init schema:18959: `ALTER TABLE public.external_threads ADD CONSTRAINT external_threads_target_principal_fk FOREIGN KEY (company_id, principal_id) REFERENCES public.principals(company_id, id);`

## internal_notes
- init schema:9846: `ALTER TABLE ONLY public.internal_notes ADD CONSTRAINT internal_notes_supersedes_fk FOREIGN KEY (company_id, supersedes_note_id) REFERENCES public.internal_notes(company_id, id) ON DELETE CASCADE;`
- init schema:9854: `ALTER TABLE ONLY public.internal_notes ADD CONSTRAINT internal_notes_thread_fk FOREIGN KEY (company_id, channel_id, thread_id) REFERENCES public.threads(company_id, channel_id, id) ON DELETE CASCADE;`
- init schema:18960: `ALTER TABLE public.internal_notes ADD CONSTRAINT internal_notes_thread_fk_principal_fk FOREIGN KEY (company_id, principal_id, thread_id) REFERENCES public.threads(company_id, principal_id, id) ON DELETE CASCADE;`
- init schema:18962: `ALTER TABLE public.internal_notes ADD CONSTRAINT internal_notes_target_principal_fk FOREIGN KEY (company_id, principal_id) REFERENCES public.principals(company_id, id);`

## message_deliveries
- init schema:9869: `ALTER TABLE ONLY public.message_deliveries ADD CONSTRAINT message_deliveries_channel_fk FOREIGN KEY (company_id, channel_id) REFERENCES public.channels(company_id, id) ON DELETE CASCADE;`
- init schema:9877: `ALTER TABLE ONLY public.message_deliveries ADD CONSTRAINT message_deliveries_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;`
- init schema:9893: `ALTER TABLE ONLY public.message_deliveries ADD CONSTRAINT message_deliveries_destination_binding_fk FOREIGN KEY (company_id, channel_id, destination_binding_id, transport) REFERENCES public.channel_bindings(company_id, channel_id, id, transport) ON DELETE CASCADE;`
- init schema:9901: `ALTER TABLE ONLY public.message_deliveries ADD CONSTRAINT message_deliveries_message_fk FOREIGN KEY (company_id, message_id, message_audience) REFERENCES public.messages(company_id, id, audience) ON DELETE CASCADE;`
- init schema:9925: `ALTER TABLE ONLY public.message_deliveries ADD CONSTRAINT message_deliveries_source_binding_fk FOREIGN KEY (company_id, channel_id, source_binding_id) REFERENCES public.channel_bindings(company_id, channel_id, id) ON DELETE CASCADE;`
- init schema:18963: `ALTER TABLE public.message_deliveries ADD CONSTRAINT message_deliveries_destination_binding_fk_principal_fk FOREIGN KEY (company_id, principal_id, destination_binding_id, transport) REFERENCES public.channel_bindings(company_id, principal_id, id, transport) ON DELETE CASCADE;`
- init schema:18964: `ALTER TABLE public.message_deliveries ADD CONSTRAINT message_deliveries_source_binding_fk_principal_fk FOREIGN KEY (company_id, principal_id, source_binding_id) REFERENCES public.channel_bindings(company_id, principal_id, id) ON DELETE CASCADE;`
- init schema:18965: `ALTER TABLE public.message_deliveries ADD CONSTRAINT message_deliveries_target_principal_fk FOREIGN KEY (company_id, principal_id) REFERENCES public.principals(company_id, id);`

## Existing application lock prefix

- `thread/inbound.rs:132–200`: commit validates target alignment, obtains company KEY SHARE and requires a row before identity/principal/binding/provider/thread effects.
- `thread/inbound_target.rs:67–128`: identity keys before sorted principal SHARE locks, then current revision/address/participation revalidation.
- `company.rs:104–138`: company FOR UPDATE first, integration/memory retirement, company DELETE and transaction commit.
- Existing private notice protocol uses its accepted NOWAIT locking path; D2 does not alter it.

## Existing negative test

- `address_tests/validation.rs:184–251`: wrong-tenant thread INSERT reaches threads_target_principal_fk; separate wrong-target message and membership cases reach their immediate composite thread FKs. Autocommit still checks a deferred pin at statement transaction commit; explicit transactions must test COMMIT or force that specific constraint immediate.
