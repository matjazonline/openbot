# CH-P01-AG3-R1 source inspection

Architecture review only. Source identity: b6c8d018ef4d17eabb1808b1cf4ee5672e659d89844f6aa6333dfd4dcf0d5aae.
These excerpts are current production contracts, not B3 implementation or executed behavior.

## integrations/cloudroom-plugin/src/sdk.ts:168
```
168:     }
169:     if(command.kind==='create' || command.kind==='send') {
170:       const harness=command.kind==='create'?command.harness:this.journal.session(command.session_id)!.harness;
171:       if(harness.kind==='agent') {
172:         if(!this.bundleReader || !this.requiredRecheck) throw new PreflightRefusal('recovery_required');
173:         const loaded=await this.nativeCache.load(this.bundleReader,record.operation.id,harness,command.kind==='create'?'admission':'required_configuration',signal);
174:         if(loaded.bundle.permissions!==selection.policy.permissions) throw new PreflightRefusal('permissions_unavailable');
175:         if(command.kind==='send') this.journal.referenceOperation(command.session_id,record.operation.id);
```

## integrations/cloudroom-plugin/src/sdk.ts:198
```
198:     } else if (command.kind === 'send') {
199:       const result = await this.sdk.threads.send({ threadId: command.thread_id, mode: 'queue-if-active', model: prepared.selection.model,
200:         permissionMode: nativePermission(prepared.selection.policy.permissions), reasoningLevel: prepared.selection.policy.reasoning, input: [{ type: 'text', text: record.sdkInput, mentions: [] }], signal });
201:       record.threadId = command.thread_id;
202:       if (result.delivery === 'queued') record.queueId = result.queuedMessage.id;
203:       this.journal.submission(record, prepared.selection, result.delivery === 'queued' ? 'queued' : 'accepted');
```

## integrations/cloudroom-plugin/src/journal.ts:223
```
223:   session(id: string): SessionMapping | undefined {
224:     const row = this.db.prepare('SELECT mapping FROM sessions WHERE id=?').get(id) as {mapping:string}|undefined;
225:     return row ? mappingSchema.parse(JSON.parse(row.mapping)) : undefined;
226:   }
227:   thread(id: string): SessionMapping | undefined {
228:     const row = this.db.prepare('SELECT mapping FROM sessions WHERE thread_id=?').get(id) as {mapping:string}|undefined;
229:     return row ? mappingSchema.parse(JSON.parse(row.mapping)) : undefined;
230:   }
231:   referenceOperation(session: string, operation: string): void {
232:     const mapping = this.session(session); if (!mapping) throw new RecoveryRequired();
233:     mapping.operationId = uuid.parse(operation);
234:     this.db.prepare('UPDATE sessions SET mapping=?,last_used_at=max(last_used_at+1,?) WHERE id=?').run(JSON.stringify(mapping),Date.now(),session);
235:   }
```

## integrations/cloudroom-plugin/src/dispatcher.ts:23
```
23:   private blocked(record: CommandRecord, context: DispatchContext): Refusal | undefined {
24:     if (Date.parse(record.operation.deadline) <= Date.now()) return 'expired';
25:     if (this.journal.recoveryRequired() || !context.canDispatch() || context.signal.aborted) return 'recovery_required';
26:     const counts = this.journal.capacity(); const effect = isEffect(record.operation.command);
27:     if (counts.queued > (effect ? LIMITS.queued - LIMITS.controls : LIMITS.queued)
28:         || (effect && counts.effects > LIMITS.effects)) return 'capacity';
29:     return undefined;
30:   }
```

## src/adapters/persistence/local_agents/protocol/observations.rs:288
```
288:     }
289:     mutation.dispatch = DispatchEvidence::NativeIdentity {
290:         sequence: observation.sequence,
291:         thread_id: thread_id.clone(),
292:         turn_id: Some(turn_id.clone()),
293:     };
294:     mutation.state = Some(*state);
295:     mutation.outcome = match state {
296:         TurnState::Completed => Some(OperationOutcome::Succeeded),
297:         TurnState::Failed => Some(OperationOutcome::Failed),
298:         TurnState::Stopped => Some(OperationOutcome::Cancelled),
299:         TurnState::Missing => Some(OperationOutcome::Uncertain),
```

## src/adapters/persistence/workflow/action_reconciliation/mod.rs:198
```
198: async fn restore_on(
199:     tx: &mut Transaction<'_, Postgres>,
200:     command: &ReconcileActionCommand,
201: ) -> AppResult<ActionRunAuthority> {
202:     let run = sqlx::query_as::<_, FrozenRunRow>("SELECT actor_id,version_id,workflow_id,stored.bundle AS bundle,resources FROM workflow_runs AS run LEFT JOIN workflow_bundles AS stored ON stored.company_id=run.company_id AND stored.digest=run.bundle_digest WHERE run.company_id=$1 AND id=$2")
203:         .bind(command.scope.company.as_uuid()).bind(command.scope.run.as_uuid()).fetch_one(&mut **tx).await?;
204:     sqlx::query_scalar::<_, Uuid>(
205:         "SELECT id FROM workflow_executions WHERE company_id=$1 AND run_id=$2 AND id=$3 FOR UPDATE",
206:     )
207:     .bind(command.scope.company.as_uuid())
208:     .bind(command.scope.run.as_uuid())
209:     .bind(command.scope.execution.as_uuid())
210:     .fetch_optional(&mut **tx)
211:     .await?
212:     .ok_or_else(missing)?;
213:     sqlx::query_scalar::<_, Uuid>("SELECT id FROM background_tasks WHERE company_id=$1 AND workflow_execution_id=$2  FOR UPDATE")
214:         .bind(command.scope.company.as_uuid()).bind(command.scope.execution.as_uuid()).fetch_optional(&mut **tx).await?.ok_or_else(missing)?;
215:     let operation: Value = sqlx::query_scalar("SELECT operation FROM workflow_action_intents WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND id=$4 AND argument_digest=$5 FOR SHARE")
216:         .bind(command.scope.company.as_uuid()).bind(command.scope.run.as_uuid()).bind(command.scope.execution.as_uuid())
217:         .bind(command.subject.invocation.as_uuid()).bind(command.subject.argument_digest.as_str())
218:         .fetch_optional(&mut **tx).await?.ok_or_else(missing)?;
219:     let action = FrozenAction::restore(operation)?;
220:     let bundle = restore_bundle(
221:         &run.bundle,
222:         &crate::adapters::workflow_source::WorkflowSourceDecoder,
223:     )?;
224:     if bundle.compiled().graph().definition().version_id.as_uuid() != run.version_id
225:         || bundle.compiled().graph().definition().workflow_id.as_uuid() != run.workflow_id
226:     {
227:         return Err(invalid());
228:     }
229:     let actor = WorkflowActor::authenticated(run.actor_id)?;
230:     let origin = super::agent_origin::authority_on(tx, action.request(), actor, &bundle).await?;
231:     let frozen = ActionRunAuthority {
232:         origin,
233:         action,
234:         actor: WorkflowActor::authenticated(run.actor_id)?,
235:         bundle,
236:         resources: serde_json::from_value(run.resources).map_err(|_| invalid())?,
237:     };
238:     validate_run_authority(command.scope, &command.subject, &frozen)?;
239:     Ok(frozen)
240: }
```

## src/adapters/persistence/workflow/agent_origin.rs:12
```
12: pub(super) async fn authority_on(
13:     db: &mut PgConnection,
14:     request: &ActionRequest,
15:     actor: WorkflowActor,
16:     bundle: &PublishedBundle,
17: ) -> AppResult<ActionOriginAuthority> {
18:     match request.context.origin {
19:         ToolCallOrigin::Workflow => {
20:             require_workflow_origin(db, request.scope, bundle).await?;
21:             Ok(ActionOriginAuthority::Workflow)
22:         }
23:         ToolCallOrigin::Agent(origin) => {
24:             let selection = selected_on(db, request.scope, actor, bundle, origin).await?;
25:             validate_agent_action(request, &selection)?;
26:             Ok(ActionOriginAuthority::Agent(Box::new(selection)))
27:         }
28:         ToolCallOrigin::ChannelFinal(_) => Err(missing()),
29:     }
30: }
31: 
32: /// Prevent a missing agent origin from silently falling through to STEP/global authority.
33: pub(super) async fn require_workflow_origin(
34:     db: &mut PgConnection,
35:     scope: ActionScope,
36:     bundle: &PublishedBundle,
37: ) -> AppResult<()> {
38:     let step: String = sqlx::query_scalar(
39:         "SELECT step_id FROM workflow_executions WHERE company_id=$1 AND run_id=$2 AND id=$3",
40:     )
41:     .bind(scope.company.as_uuid())
42:     .bind(scope.run.as_uuid())
43:     .bind(scope.execution.as_uuid())
44:     .fetch_optional(db)
45:     .await?
46:     .ok_or_else(missing)?;
47:     let step = StepId::parse(step).map_err(|_| invalid())?;
48:     let definition = bundle
49:         .compiled()
50:         .graph()
51:         .definition()
52:         .steps
53:         .get(&step)
54:         .ok_or_else(missing)?;
55:     if crate::application::workflow::registry::ModelStep::of(definition.step_type.as_str())
56:         .is_some()
57:     {
58:         return Err(missing());
59:     }
60:     Ok(())
61: }
```

## src/adapters/persistence/workflow/action_dispatch.rs:96
```
96:     pub(super) async fn authorize_prelocked_on(
97:         &self,
98:         tx: &mut Transaction<'_, Postgres>,
99:         request: &ActionDispatchRequest,
100:         server: Duration,
101:         actor: Option<WorkflowActor>,
102:     ) -> AppResult<AuthorizedAction> {
103:         if !lease::lock_fence(tx, request.fence, server).await? {
104:             return Err(conflict());
105:         }
106:         // The existing attempt remains the sole ownership ledger.
107:         let attempt: Option<Uuid> = sqlx::query_scalar(
108:             "SELECT id FROM task_attempts WHERE task_id=$1 AND attempt_number=$2 \
109:              AND execution_generation=$3 AND worker_id=$4 AND status='processing' FOR UPDATE",
110:         )
111:         .bind(request.fence.scope.job.0)
112:         .bind(request.fence.attempt.0)
113:         .bind(request.fence.generation.0)
114:         .bind(request.fence.worker.0)
115:         .fetch_optional(&mut **tx)
116:         .await?;
117:         if attempt.is_none()
118:             || lease::live_window(tx, request.fence, Instant::now())
119:                 .await?
120:                 .is_none()
121:         {
122:             return Err(conflict());
123:         }
124:         let authority = Box::pin(action_authority::load_on(
125:             tx,
126:             request.scope(),
127:             &request.subject,
128:             actor,
129:             action_authority::RunPhase::Claimed,
130:         ))
131:         .await?;
132:         validate_run_authority(request.scope(), &request.subject, &authority)?;
133:         let current = self.authority.lock_current(tx, &authority).await?;
134:         validate_resource(authority.action.request(), &current.resource)?;
135:         let decision = validate_current_authority(authority.action.request(), &current.policy)?;
136:         // Lock the exact immutable intent after all authoritative owners. No caller
```

## src/adapters/persistence/workflow/lease.rs:66
```
66:     async fn renew_io(
67:         &self,
68:         fence: WorkflowFence,
69:         policy: LeasePolicy,
70:     ) -> AppResult<Option<LeaseWindow>> {
71:         let bound = WriteBound::start("lease.renew", policy);
72:         let started = bound.started();
73:         let attempt = bound
74:             .attempt(Box::pin(async {
75:                 let mut conn = bound.acquire(&self.pool).await?;
76:                 let mut tx = bound.begin(&mut conn).await?;
77:                 let renewed: AppResult<End<_>> = async {
78:                     if !lock_fence(&mut tx, fence, bound.server_timeout()).await? {
79:                         return Ok(End::Rollback(None));
80:                     }
81:                     // A renewal must not remain uncommitted across its old expiry while
82:                     // another claimant counts that old version as free capacity.
83:                     capacity::lock(&mut tx).await?;
84:                     super::persistence_bound::locks_held();
85:                     if live_window(&mut tx, fence, started).await?.is_none() {
86:                         return Ok(End::Rollback(None));
87:                     }
88:                     renew_owned(&mut tx, fence, policy).await?;
89:                     Ok(End::Commit(live_window(&mut tx, fence, started).await?))
90:                 }
91:                 .await;
```

## src/adapters/persistence/workflow/lease.rs:291
```
291: pub(super) async fn lock_scope(
292:     db: &mut PgConnection,
293:     scope: ActivationRequest,
294:     timeout: std::time::Duration,
295: ) -> AppResult<Option<(LeaseRun, LeaseJob)>> {
296:     set_server_timeouts(db, timeout).await?;
297:     channel_reply_completion::prelock_on(db, scope).await?;
298:     // Run-first ordering serializes claims, renewals, cancellation and progression.
299:     let run = sqlx::query_as::<_, LeaseRun>("SELECT binding_id, max_context_bytes FROM workflow_runs WHERE company_id=$1 AND id=$2 AND state IN ('queued','running') FOR UPDATE")
300:         .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_optional(&mut *db).await?;
301:     let Some(run) = run else {
302:         return Ok(None);
303:     };
304:     let step: Option<String> = sqlx::query_scalar("SELECT step_id FROM workflow_executions WHERE company_id=$1 AND run_id=$2 AND id=$3 AND completed_at IS NULL FOR UPDATE")
305:         .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).fetch_optional(&mut *db).await?;
306:     let Some(step_id) = step else {
307:         return Ok(None);
308:     };
309:     let job = sqlx::query_as::<_, LeaseJob>("SELECT status,retry_count,attempt_ordinal,max_retries,worker_id,execution_generation,lock_expires_at,$4::text AS step_id FROM background_tasks WHERE company_id=$1 AND id=$2 AND workflow_execution_id=$3  FOR UPDATE")
310:         .bind(scope.company.as_uuid()).bind(scope.job.0).bind(scope.execution.as_uuid()).bind(step_id).fetch_optional(&mut *db).await?;
311:     let Some(job) = job else {
312:         return Ok(None);
313:     };
314:     super::persistence_bound::locks_held();
315:     let active: bool = sqlx::query_scalar("SELECT state IN ('queued','running') AND deadline > clock_timestamp() FROM workflow_runs WHERE company_id=$1 AND id=$2")
316:         .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_one(&mut *db).await?;
317:     Ok(active.then_some((run, job)))
318: }
```

## src/adapters/persistence/local_agents/protocol/observations.rs:27
```
27: pub(super) async fn accept(
28:     p: &PostgresPersistence,
29:     fence: ConnectionFence,
30:     observation: &JournalObservation,
31: ) -> AppResult<u64> {
32:     let digest = observation.canonical_digest()?;
33:     let sequence = observation.sequence().get();
34:     let operation = observation.operation_id();
35:     let mut tx = begin(&p.pool).await?;
36:     let machine = lock_fence(&mut tx, fence).await?;
37:     if machine.journal_generation != fence.generation.get() as i64 {
38:         return Err(AppError::InvalidCredentials);
39:     }
40:     let acknowledged = machine.acknowledged_sequence as u64;
41:     if sequence <= acknowledged {
42:         let old:Option<String>=sqlx::query_scalar("SELECT digest FROM local_agent_observations WHERE company_id=$1 AND installation_id=$2 AND sequence=$3")
43:             .bind(fence.machine.company_id).bind(fence.machine.installation_id.as_uuid()).bind(sequence as i64).fetch_optional(&mut *tx).await?;
44:         if old.as_deref() != Some(digest.as_str()) {
45:             return Err(AppError::Conflict(
46:                 "journal sequence content changed".into(),
47:             ));
48:         }
49:         reconcile_generation(&mut tx, fence, operation).await?;
50:         commit_cursor(&mut tx, fence, acknowledged).await?;
51:         tx.commit().await?;
52:         return Ok(acknowledged);
53:     }
54:     if sequence != acknowledged + 1 {
55:         return Err(AppError::Conflict("journal sequence gap".into()));
56:     }
57:     let row=sqlx::query_as::<_,OperationDb>(&format!("SELECT {} FROM local_agent_operations WHERE company_id=$1 AND machine_id=$2 AND installation_id=$3 AND id=$4 FOR UPDATE", operation_columns()))
58:         .bind(fence.machine.company_id).bind(fence.machine.machine_id.as_uuid()).bind(fence.machine.installation_id.as_uuid()).bind(operation.as_uuid())
59:         .fetch_optional(&mut *tx).await?.ok_or_else(missing)?;
60:     if row.delivery_attempted_at.is_none() {
61:         return Err(AppError::BadRequest(
62:             "observation precedes possible delivery".into(),
63:         ));
64:     }
65:     let original = row.operation()?;
66:     let mut mutation = apply(&original, observation)?;
```

## src/adapters/persistence/local_agents/protocol/mod.rs:290
```
290:     async fn expire_operations(&self, limit: u16) -> AppResult<u16> {
291:         if limit == 0 || limit > 100 {
292:             return Err(AppError::BadRequest("invalid expiration batch".into()));
293:         }
294:         let mut tx = begin(&self.pool).await?;
295:         let result=sqlx::query(&format!("WITH due AS (SELECT company_id,id FROM local_agent_operations WHERE outcome IS NULL AND deadline<=clock_timestamp() ORDER BY deadline,id LIMIT $1 FOR UPDATE SKIP LOCKED) \
296:             UPDATE local_agent_operations AS operation SET outcome={EXPIRATION_OUTCOME_SQL} \
297:             FROM due WHERE operation.company_id=due.company_id AND operation.id=due.id AND operation.outcome IS NULL"))
298:             .bind(i64::from(limit)).execute(&mut *tx).await?;
299:         tx.commit().await?;
300:         Ok(result.rows_affected() as u16)
301:     }
```

## src/adapters/persistence/workflow/admission.rs:11
```
11:     async fn admit(&self, command: &PreparedAdmission) -> AppResult<AdmissionResult> {
12:         agent_history_sessions::check_manual(command)?;
13:         let mut tx = self.pool.begin().await?;
14:         if command.admission_claim().is_some() {
15:             sqlx::query("SET LOCAL lock_timeout = '5s'")
16:                 .execute(&mut *tx)
17:                 .await?;
18:             sqlx::query("SET LOCAL statement_timeout = '20s'")
19:                 .execute(&mut *tx)
20:                 .await?;
21:         }
22:         authority::authorize_company(&mut tx, command.company_id(), command.actor()).await?;
23:         association::authorize_association(
24:             &mut tx,
25:             command.company_id(),
26:             command.actor(),
27:             command.association(),
28:         )
29:         .await?;
30:         admission_source::authorize(&mut tx, command).await?;
31:         if !claim_holds(&mut tx, command).await? {
32:             tx.rollback().await?;
33:             return Ok(AdmissionResult::Stale);
34:         }
35:         let source = SourceKey::from_trigger(command.trigger());
36:         if let Some(result) = admission_replay::resolve(&mut tx, command, &source).await? {
37:             if let AdmissionResult::Replayed(run) = result
38:                 && !settle_claim(&mut tx, command, run).await?
39:             {
40:                 tx.rollback().await?;
41:                 return Ok(AdmissionResult::Stale);
42:             }
43:             tx.commit().await?;
44:             return Ok(result);
45:         }
46:         if !verify_current(&mut tx, command).await? {
47:             // Nothing was written; the caller re-reads the binding and decides again.
48:             tx.rollback().await?;
49:             return Ok(AdmissionResult::Stale);
50:         }
51:         authorize_token_reviewers(&mut tx, command).await?;
52:         resources::check_readiness(&mut tx, command.binding().as_ref().into()).await?;
53:         admission_write::create(&mut tx, command, &source).await?;
54:         if !settle_claim(&mut tx, command, command.proposed_run_id()).await? {
55:             tx.rollback().await?;
56:             return Ok(AdmissionResult::Stale);
57:         }
58:         tx.commit().await?;
59:         tracing::info!("Workflow admission committed");
60:         Ok(AdmissionResult::Created(command.proposed_run_id()))
61:     }
```

## src/adapters/persistence/workflow/admission_write.rs:7
```
7: pub(super) async fn create(
8:     tx: &mut Transaction<'_, Postgres>,
9:     command: &PreparedAdmission,
10:     source: &SourceKey,
11: ) -> AppResult<()> {
12:     let manifest = super::agent_manifest::capture_on(tx, command).await?;
13:     let knowledge = super::knowledge::admission_levels_on(tx, command).await?;
14:     insert_run(
15:         tx,
16:         &RunRow {
17:             company: command.company_id(),
18:             knowledge: knowledge.levels,
19:             content_exact: knowledge.exact,
20:             run: command.proposed_run_id(),
21:             binding: command.binding(),
22:             association: command.association(),
23:             actor_id: command.actor().user_id(),
24:             trigger: command.trigger().trigger_id(),
25:             correlation: command.causality().correlation_id().as_uuid(),
26:             input: command.input(),
27:             deadline: RunDeadline::AdmissionWindow,
28:             parent: None,
29:             rerun_of: command.rerun_of(),
30:             delivery_policy: command.delivery_policy(),
31:         },
32:     )
33:     .await?;
34:     super::agent_manifest::persist_on(tx, &manifest).await?;
35:     insert_first_job(
36:         tx,
37:         &FirstJob {
38:             company: command.company_id(),
39:             run: command.proposed_run_id(),
40:             execution: command.first_execution_id(),
41:             entry: command.entry(),
42:             association: command.association(),
43:             correlation: command.causality().correlation_id().as_uuid(),
44:         },
45:     )
46:     .await?;
47:     admission_history::capture(tx, command).await?;
48:     insert_admission(
49:         tx,
50:         command.company_id(),
51:         command.binding().id(),
52:         source,
53:         command.proposed_run_id(),
54:     )
55:     .await?;
56:     admission_replay::insert_command(tx, command, command.proposed_run_id().as_uuid()).await?;
57:     insert_admitted(
58:         tx,
59:         command.company_id(),
60:         command.proposed_run_id(),
61:         command.actor().user_id(),
62:     )
63:     .await?;
64:     agent_history_sessions::capture(tx, command).await
65: }
```

## src/adapters/persistence/workflow/knowledge.rs:116
```
116: pub(super) async fn admission_levels_on(
117:     db: &mut PgConnection,
118:     command: &PreparedAdmission,
119: ) -> AppResult<AdmittedKnowledge> {
120:     let company = command.company_id();
121:     let (levels, exact) = match command.trigger().source() {
122:         TriggerSource::Manual => (
123:             RunKnowledge::new(
124:                 actor_level_on(db, company, Some(command.actor().user_id())).await?,
125:                 AccessLevel::PUBLIC,
126:             ),
127:             human_input_exactness(HumanInputClass::ConversationalInput, ExactnessTarget::Run),
128:         ),
129:         TriggerSource::Schedule { .. } => (
130:             RunKnowledge::new(
131:                 actor_level_on(db, company, Some(command.binding().run_as().user_id())).await?,
132:                 AccessLevel::PUBLIC,
133:             ),
134:             human_input_exactness(HumanInputClass::AuthoredConfiguration, ExactnessTarget::Run),
135:         ),
136:         TriggerSource::Message { message_id } => {
137:             let ceiling = thread_audience_on(db, company, command.association()).await?;
```

## pinned Cloudroom public PluginAgentToolContext:1113
```
1113: export interface PluginAgentToolContext {
1114:   threadId: string;
1115:   projectId: string;
1116:   /**
1117:    * Aborts when the tool-call request is cancelled, the thread is stopped or
1118:    * deleted, or this plugin is disposed. Opening a form with `bb.ui.requestInput`
1119:    * detaches the call from its request: the agent receives a waiting notice,
1120:    * and request cancellation no longer aborts this signal. The tool's eventual
1121:    * result is delivered as a system message (success steers a running turn or
1122:    * starts a new one; an `isError` result only steers). Thread stop/delete and
1123:    * plugin disposal still abort the signal after detachment.
1124:    */
1125:   signal: AbortSignal;
1126: }
```
