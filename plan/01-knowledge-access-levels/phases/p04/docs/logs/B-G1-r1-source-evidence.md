# KNOW-P04-G1-r1 independent source evidence

Selected actual baseline spans inspected by B; source is pristine. These establish extension points, not the correctness of future implementation.

## src/adapters/persistence/workflow/checkpoint_updates.rs

Whole-file SHA256: 0a0847c4828916af5dc5a143302a591d6354f23ad309e1b5293be3dfa8cf681c

Lines 4–61:
```text
4: pub(super) async fn update_on(
5:     tx: &mut Transaction<'_, Postgres>,
6:     write: WorkflowCheckpointWrite,
7:     update: WorkflowCheckpointUpdate,
8:     bound: &WriteBound,
9: ) -> AppResult<End<Outcome>> {
10:     // A saved call is dispatched, or re-dispatched after approval, only under
11:     // current agent/model authority; replay of the preparation rechecks it too.
12:     if matches!(update, WorkflowCheckpointUpdate::PrepareInvocation(_)) {
13:         authorize_on(tx, write.fence).await?;
14:     }
15:     if !lock_owner(tx, write.fence, bound).await? {
16:         return Ok(End::Rollback(WriteOutcome::OwnershipLost));
17:     }
18:     let saved = load_on(tx, write.fence).await?.ok_or_else(missing)?;
19:     let current = decode(saved.id, saved.checkpoint.clone())?;
20:     live(tx, write.fence, bound).await?;
21:     if current.run_id != write.checkpoint {
22:         return Ok(End::Rollback(WriteOutcome::OwnershipLost));
23:     }
24:     let receives_truth = matches!(&update, WorkflowCheckpointUpdate::InvocationResult { .. });
25:     let (next, changed) = match current.apply(mutation(update)) {
26:         Ok(result) => result,
27:         Err(_) if current.revision != write.expected_revision => {
28:             return Ok(End::Rollback(WriteOutcome::RevisionConflict));
29:         }
30:         Err(error) => return Err(error),
31:     };
32:     if !changed {
33:         return Ok(End::Rollback(WriteOutcome::AlreadyApplied(current)));
34:     }
35:     if current.revision != write.expected_revision {
36:         return Ok(End::Rollback(WriteOutcome::RevisionConflict));
37:     }
38:     if saved.model_request_id.is_some() {
39:         return Err(AppError::Conflict("Model request is still active".into()));
40:     }
41:     if !current
42:         .executions
43:         .iter()
44:         .any(|execution| execution.generation == write.fence.generation.0)
45:     {
46:         return Err(AppError::Conflict(
47:             "Current execution time has not been reserved".into(),
48:         ));
49:     }
50:     if let Some(decision) = saved.model_blocked_decision {
51:         approved_settlement(tx, write.fence.scope, decision).await?;
52:     }
53:     // A received result is durable truth, not permission for another effect.
54:     // The next prepare/final/model boundary rechecks the shared root gate.
55:     if !receives_truth || saved.model_blocked_decision.is_some() {
56:         token_budget::require_progression(tx, write.fence.scope).await?;
57:     }
58:     persist_on(tx, write.fence, &next, CheckpointOwnership::Progression).await?;
59:     live(tx, write.fence, bound).await?;
60:     Ok(End::Commit(WriteOutcome::Applied(next)))
61: }
```

## src/adapters/persistence/workflow/checkpoints.rs

Whole-file SHA256: b9e35c88bff57e406af592950e846fa10116f03a3e6875f9e0b42e292a424e03

Lines 268–338:
```text
268: async fn lock_owner(
269:     tx: &mut Transaction<'_, Postgres>,
270:     fence: WorkflowFence,
271:     bound: &WriteBound,
272: ) -> AppResult<bool> {
273:     let scope = ActionScope {
274:         company: fence.scope.company,
275:         run: fence.scope.run,
276:         execution: fence.scope.execution,
277:     };
278:     let Some(actor) = action_authority::prelock_on(tx, scope).await? else {
279:         return Ok(false);
280:     };
281:     if !lease::lock_fence(tx, fence, bound.server_timeout()).await? {
282:         return Ok(false);
283:     }
284:     let current: Uuid =
285:         sqlx::query_scalar("SELECT actor_id FROM workflow_runs WHERE company_id=$1 AND id=$2")
286:             .bind(scope.company.as_uuid())
287:             .bind(scope.run.as_uuid())
288:             .fetch_one(&mut **tx)
289:             .await?;
290:     Ok(current == actor.user_id()
291:         && lease::live_window(tx, fence, bound.started())
292:             .await?
293:             .is_some())
294: }
295: 
296: async fn live(tx: &mut PgConnection, fence: WorkflowFence, bound: &WriteBound) -> AppResult<()> {
297:     if lease::live_window(tx, fence, bound.started())
298:         .await?
299:         .is_none()
300:     {
301:         return Err(AppError::Execution(
302:             crate::app_error::ExecutionFailure::OwnershipLost,
303:         ));
304:     }
305:     Ok(())
306: }
307: 
308: /// Current agent/model authority for the immutable prepared selection, taken in the
309: /// authority class before any local workflow lock. Missing preparation fails closed.
310: async fn authorize_on(tx: &mut PgConnection, fence: WorkflowFence) -> AppResult<()> {
311:     let prepared: Value = sqlx::query_scalar(
312:         "SELECT workflow_prepared FROM task_harness_runs WHERE company_id=$1
313:           AND workflow_run_id=$2 AND workflow_execution_id=$3 AND task_id=$4",
314:     )
315:     .bind(fence.scope.company.as_uuid())
316:     .bind(fence.scope.run.as_uuid())
317:     .bind(fence.scope.execution.as_uuid())
318:     .bind(fence.scope.job.0)
319:     .fetch_optional(&mut *tx)
320:     .await?
321:     .ok_or_else(missing)?;
322:     super::agent_preparation::current_on(tx, fence.scope.company, &prepared).await
323: }
324: 
325: async fn load_on(tx: &mut PgConnection, fence: WorkflowFence) -> AppResult<Option<Saved>> {
326:     sqlx::query_as(
327:         "SELECT id,checkpoint,workflow_prepared,model_blocked_decision,model_request_id,model_generation,model_worker_id,model_attempt FROM task_harness_runs
328:         WHERE company_id=$1 AND workflow_run_id=$2
329:           AND workflow_execution_id=$3 AND task_id=$4 FOR UPDATE",
330:     )
331:     .bind(fence.scope.company.as_uuid())
332:     .bind(fence.scope.run.as_uuid())
333:     .bind(fence.scope.execution.as_uuid())
334:     .bind(fence.scope.job.0)
335:     .fetch_optional(tx)
336:     .await
337:     .map_err(Into::into)
338: }
```

## src/application/services/harness/runs/state.rs

Whole-file SHA256: 5dbced6233f051b98633b99a046d2ce9613a1e16fdf58fcbeea9c6efde24f70f

Lines 715–757:
```text
715:     fn result(&mut self, id: InvocationId, result: Value) -> AppResult<bool> {
716:         if result.to_string().len() > MAX_RESULT_BYTES {
717:             return Err(invalid("Tool result exceeds checkpoint bound"));
718:         }
719:         if let Some(saved) = self.invocations.iter().find(|saved| {
720:             saved.call.invocation_id == id && saved.state == InvocationState::Completed
721:         }) {
722:             return if saved.result.as_ref() == Some(&result) {
723:                 Ok(false)
724:             } else {
725:                 Err(invalid("Invocation result identity collision"))
726:             };
727:         }
728:         self.require_active()?;
729:         if self
730:             .pending_call()
731:             .is_none_or(|call| call.invocation_id != id)
732:         {
733:             return Err(invalid("Result is not for the next saved call"));
734:         }
735:         let invocation = self
736:             .invocations
737:             .iter_mut()
738:             .find(|saved| {
739:                 saved.call.invocation_id == id
740:                     && matches!(
741:                         saved.state,
742:                         InvocationState::Prepared
743:                             | InvocationState::Ready
744:                             | InvocationState::Indeterminate
745:                     )
746:             })
747:             .ok_or_else(|| invalid("Invocation was not prepared"))?;
748:         self.messages.push(ConversationMessage::Tool {
749:             invocation_id: id,
750:             call_id: invocation.call.call_id.clone(),
751:             item_id: invocation.call.item_id.clone(),
752:             result: result.clone(),
753:         });
754:         invocation.result = Some(result);
755:         invocation.state = InvocationState::Completed;
756:         Ok(true)
757:     }
```

## src/adapters/harness/rig/workflow.rs

Whole-file SHA256: 60addd1c2a04b6d9bf1b11be19af4b7c9083018027766b7fce64a531bca148d5

Lines 64–74:
```text
64:     pub(super) fn definitions(&self) -> Vec<ToolDefinition> {
65:         self.prepared
66:             .tools()
67:             .iter()
68:             .map(|tool| ToolDefinition {
69:                 name: tool.declared.clone(),
70:                 description: format!("Workflow tool {}", tool.name),
71:                 parameters: tool.input_schema.clone(),
72:             })
73:             .collect()
74:     }
```

Lines 174–218:
```text
174:     pub(super) async fn tool(
175:         &self,
176:         checkpoint: &RunCheckpoint,
177:         call: SavedToolCall,
178:     ) -> AppResult<Progress> {
179:         let invocation = call.invocation_id;
180:         let prepared = self
181:             .update(
182:                 checkpoint,
183:                 WorkflowCheckpointUpdate::PrepareInvocation(invocation),
184:             )
185:             .await?;
186:         // Only prepared declarations dispatch; a model-invented name or target never does.
187:         let Some(tool) = self.prepared.tool(call.tool_id.as_str()) else {
188:             let refused = json!({"error":{"code":"unknown_tool","message":"Tool is not available to this agent"}});
189:             return Ok(Progress::Continue(
190:                 self.update(
191:                     &prepared,
192:                     WorkflowCheckpointUpdate::InvocationResult {
193:                         invocation,
194:                         result: refused,
195:                     },
196:                 )
197:                 .await?,
198:             ));
199:         };
200:         let id = ModelToolCallId::parse(invocation.0.to_string())?;
201:         match self
202:             .tools
203:             .dispatch(self.fence, tool, id, call.arguments)
204:             .await?
205:         {
206:             AgentToolObservation::Completed(result) => Ok(Progress::Continue(
207:                 self.update(
208:                     &prepared,
209:                     WorkflowCheckpointUpdate::InvocationResult { invocation, result },
210:                 )
211:                 .await?,
212:             )),
213:             AgentToolObservation::Parked(wait) => Ok(Progress::Suspend(prepared, wait)),
214:             AgentToolObservation::Uncertain => {
215:                 Err(AppError::Execution(ExecutionFailure::IndeterminateEffect))
216:             }
217:         }
218:     }
```

## src/adapters/persistence/workflow/agent_preparation.rs

Whole-file SHA256: 81ff9d6ff6f6e38eddb774857614bea34346f0210a42b8aff0dec9c5934e7329

Lines 68–106:
```text
68: pub(in crate::adapters::persistence::workflow) async fn current_on(
69:     db: &mut PgConnection,
70:     company: CompanyId,
71:     prepared: &Value,
72: ) -> AppResult<()> {
73:     let selected = Selected::read(prepared)?;
74:     let grants: Option<Vec<String>> = sqlx::query_scalar(
75:         "SELECT workflow_tools FROM agents WHERE id=$1 AND company_id=$2 FOR SHARE",
76:     )
77:     .bind(selected.agent)
78:     .bind(company.as_uuid())
79:     .fetch_optional(&mut *db)
80:     .await?;
81:     let grants = grants.ok_or_else(revoked)?;
82:     if selected.tools.iter().any(|tool| !grants.contains(tool)) {
83:         return Err(revoked());
84:     }
85:     let linked: Vec<Uuid> = sqlx::query_scalar(
86:         "SELECT skill_id FROM agent_skills WHERE agent_id=$1 AND skill_id=ANY($2)",
87:     )
88:     .bind(selected.agent)
89:     .bind(&selected.skills)
90:     .fetch_all(&mut *db)
91:     .await?;
92:     if selected.skills.iter().any(|skill| !linked.contains(skill)) {
93:         return Err(revoked());
94:     }
95:     let models: Option<Vec<String>> = sqlx::query_scalar(
96:         "SELECT models FROM company_model_connections WHERE company_id=$1 AND provider=$2 FOR SHARE",
97:     )
98:     .bind(company.as_uuid())
99:     .bind(&selected.provider)
100:     .fetch_optional(&mut *db)
101:     .await?;
102:     if !models.is_some_and(|models| models.contains(&selected.model)) {
103:         return Err(revoked());
104:     }
105:     Ok(())
106: }
```

Lines 114–162:
```text
114: async fn targets(
115:     tx: &mut Transaction<'_, Postgres>,
116:     facts: &runtime::RunFacts,
117:     company: CompanyId,
118:     tools: &[ToolSnapshot],
119: ) -> AppResult<Vec<AgentToolTarget>> {
120:     let mut targets = Vec::with_capacity(tools.len());
121:     for tool in tools {
122:         let target = match tool.kind() {
123:             ToolKind::Message => {
124:                 if MessageKind::parse(tool.contract.name.as_str()).is_none()
125:                     || !is_trusted_message_snapshot(tool)?
126:                 {
127:                     return Err(unsupported());
128:                 }
129:                 AgentToolTarget::Message {
130:                     name: tool.contract.name.clone(),
131:                 }
132:             }
133:             // Only an MCP slot is a direct agent tool; a slot-bound HTTP selection belongs to
134:             // its `http.request` step and is unsupported here (06-R016 F017).
135:             ToolKind::Slot(slot) => {
136:                 let definition = facts.bundle.compiled().graph().definition();
137:                 if super::resources::slot_kind(definition, slot) != Some("mcp")
138:                     || !facts.resources.contains_key(slot)
139:                 {
140:                     return Err(unsupported());
141:                 }
142:                 AgentToolTarget::Mcp { slot: slot.clone() }
143:             }
144:             ToolKind::Provisioning => {
145:                 if !is_trusted_provisioning_snapshot(tool)? {
146:                     return Err(unsupported());
147:                 }
148:                 AgentToolTarget::Provisioning {
149:                     name: tool.contract.name.clone(),
150:                 }
151:             }
152:             ToolKind::Http => AgentToolTarget::Http {
153:                 connection: registered_on(tx, company, tool).await?,
154:             },
155:             // Its registration is checked at each call, so a revocation refuses the call rather
156:             // than the whole agent step (07-D2 §D2.3).
157:             ToolKind::Workflow(pin) => AgentToolTarget::Workflow { pin: pin.clone() },
158:         };
159:         targets.push(target);
160:     }
161:     Ok(targets)
162: }
```

## src/adapters/persistence/workflow/agent_publication.rs

Whole-file SHA256: 2ecc2adc78ebd416a4a2c6c3862783e3ce912b9507a9f885fd0d40d8d0ce54ba

Lines 287–370:
```text
287: async fn granted_tool(
288:     tx: &mut Transaction<'_, Postgres>,
289:     company: CompanyId,
290:     name: &TypeName,
291:     captured: &[ToolSnapshot],
292:     definition: &crate::domain::workflow::WorkflowDefinition,
293: ) -> AppResult<Option<ToolSnapshot>> {
294:     let ambiguous = || {
295:         refused(format!(
296:             "Agent tool {name} conflicts with another tool of the same name in this workflow"
297:         ))
298:     };
299:     // A provisioning name resolves to the built-in before anything captured (p08 D2.1, F047): an
300:     // MCP tool of that name stays a slot tool and must never answer the grant.
301:     if let Some(kind) = ProvisioningKind::parse(name.as_str()) {
302:         let mut known = captured.iter().filter(|tool| &tool.contract.name == name);
303:         return match (known.next(), known.next()) {
304:             (None, _) => Ok(Some(builtin_provisioning_snapshot(company, kind)?)),
305:             (Some(tool), None) if is_trusted_provisioning_snapshot(tool)? => Ok(None),
306:             _ => Err(ambiguous()),
307:         };
308:     }
309:     let workflow = super::workflow_tools::workflow_tool_snapshot(tx, company, name).await?;
310:     let known: Vec<&ToolSnapshot> = captured
311:         .iter()
312:         .filter(|tool| &tool.contract.name == name)
313:         .collect();
314:     match (known.as_slice(), &workflow) {
315:         ([], _) => {}
316:         // Another agent of this workflow already captured the same workflow tool.
317:         ([tool], Some(registered)) => {
318:             return match (tool.kind(), registered.kind()) {
319:                 (ToolKind::Workflow(frozen), ToolKind::Workflow(current)) if frozen == current => {
320:                     Ok(None)
321:                 }
322:                 _ => Err(ambiguous()),
323:             };
324:         }
325:         // A built-in message tool, a registration (no slot) or a selected MCP slot is a usable
326:         // agent tool as captured.
327:         ([tool], None)
328:             if match tool.kind() {
329:                 ToolKind::Message | ToolKind::Http => true,
330:                 ToolKind::Provisioning => false,
331:                 ToolKind::Slot(slot) => {
332:                     super::resources::slot_kind(definition, slot) == Some("mcp")
333:                 }
334:                 ToolKind::Workflow(_) => false,
335:             } =>
336:         {
337:             return Ok(None);
338:         }
339:         // A slot-bound HTTP step selection is not an agent tool, and a second snapshot of
340:         // the same name would make the agent's selection ambiguous (06-R016 F017).
341:         _ => return Err(ambiguous()),
342:     }
343:     if let Some(kind) = MessageKind::parse(name.as_str()) {
344:         return Ok(Some(builtin_message_snapshot(company, kind)?));
345:     }
346:     let snapshots: Vec<Value> = sqlx::query_scalar("SELECT operation.snapshot FROM workflow_http_operations AS operation JOIN workflow_http_connections AS connection ON connection.company_id=operation.company_id AND connection.id=operation.connection_id WHERE operation.company_id=$1 AND operation.name=$2 AND connection.enabled AND (NOT connection.bearer OR EXISTS(SELECT 1 FROM workflow_http_credentials AS credential WHERE credential.company_id=connection.company_id AND credential.connection_id=connection.id)) LIMIT 2")
347:         .bind(company.as_uuid())
348:         .bind(name.as_str())
349:         .fetch_all(&mut **tx)
350:         .await?;
351:     if let Some(workflow) = workflow {
352:         if !snapshots.is_empty() {
353:             return Err(refused(format!(
354:                 "Agent tool {name} is registered both as a workflow and as an HTTP operation"
355:             )));
356:         }
357:         return Ok(Some(workflow));
358:     }
359:     match snapshots.as_slice() {
360:         [snapshot] => Ok(Some(
361:             serde_json::from_value(snapshot.clone()).map_err(|_| invalid())?,
362:         )),
363:         [] => Err(refused(format!(
364:             "Agent tool {name} has no ready HTTP registration, workflow registration or selected MCP slot"
365:         ))),
366:         _ => Err(refused(format!(
367:             "Agent tool {name} is registered on several connections"
368:         ))),
369:     }
370: }
```

## src/adapters/persistence/workflow/knowledge.rs

Whole-file SHA256: 588434d295ede2354598b0560c39a9389b5559c690d10a8f57d2968f003bc8ee

Lines 139–158:
```text
139: pub(crate) async fn raise_content_level_on(
140:     db: &mut PgConnection,
141:     company: CompanyId,
142:     run: RunId,
143:     level: AccessLevel,
144: ) -> AppResult<()> {
145:     let changed = sqlx::query(
146:         "UPDATE workflow_runs SET content_level=GREATEST(content_level,$3::smallint) \
147:          WHERE company_id=$1 AND id=$2",
148:     )
149:     .bind(company.as_uuid())
150:     .bind(run.as_uuid())
151:     .bind(level.as_i16())
152:     .execute(db)
153:     .await?;
154:     if changed.rows_affected() != 1 {
155:         return Err(missing());
156:     }
157:     Ok(())
158: }
```

## src/application/knowledge/service.rs

Whole-file SHA256: 58e704d73c44da086308fe8f8138c3527251890d138b9eee234da7c3162e00d7

Lines 27–109:
```text
27:     pub fn new(documents: Arc<dyn KnowledgeDocuments>, embedder: Arc<dyn TextEmbedder>) -> Self {
28:         Self {
29:             documents,
30:             embedder,
31:             slots: Arc::new(Semaphore::new(1)),
32:             admission: Admission::new(),
33:             tasks: TaskTracker::new(),
34:             stop: CancellationToken::new(),
35:             registration: Mutex::new(()),
36:         }
37:     }
38:     pub fn documents(&self) -> &dyn KnowledgeDocuments {
39:         self.documents.as_ref()
40:     }
41:     pub fn embedder(&self) -> Arc<dyn TextEmbedder> {
42:         self.embedder.clone()
43:     }
44:     pub fn admit_body(
45:         &self,
46:         scope: KnowledgeScope,
47:     ) -> Result<OwnedSemaphorePermit, super::admission::UploadAdmissionRefused> {
48:         if self.stop.is_cancelled() {
49:             return Err(super::admission::UploadAdmissionRefused::Busy);
50:         }
51:         self.admission.body(scope)
52:     }
53:     pub async fn upload(
54:         &self,
55:         scope: KnowledgeScope,
56:         files: Vec<KnowledgeUploadFile>,
57:     ) -> AppResult<Vec<KnowledgeDocumentLayer>> {
58:         validate_batch(&files)?;
59:         let sizes: Vec<_> = files.iter().map(KnowledgeUploadFile::size).collect();
60:         self.documents.preflight_upload(scope, &sizes).await?;
61:         let permit = self.slots.clone().try_acquire_owned().map_err(|_| {
62:             AppError::ServiceUnavailable("Knowledge indexing is busy; retry shortly".into())
63:         })?;
64:         let cancel = self.stop.child_token();
65:         let _cancel_on_drop = cancel.clone().drop_guard();
66:         let worker_cancel = cancel.clone();
67:         let model = self.embedder.clone();
68:         let deadline = Instant::now() + INDEXING_DEADLINE;
69:         // The tracker owns actual blocking work through caller cancellation and shutdown.
70:         let job = {
71:             // Serialize task registration with close/wait so shutdown cannot miss a new job.
72:             let _registration = self.registration.lock().map_err(|_| {
73:                 AppError::ServiceUnavailable("Knowledge indexing is unavailable".into())
74:             })?;
75:             live(&cancel, deadline)?;
76:             self.tasks.spawn_blocking(move || {
77:                 let _permit = permit;
78:                 index_documents(files, model.as_ref(), &worker_cancel, deadline)
79:             })
80:         };
81:         let prepared = tokio::time::timeout_at(deadline, job)
82:             .await
83:             .map_err(|_| AppError::Timeout("Knowledge indexing exceeded its deadline".into()))?
84:             .map_err(|_| {
85:                 AppError::ServiceUnavailable("Knowledge indexing could not complete".into())
86:             })??;
87:         live(&cancel, deadline)?;
88:         tokio::select! {
89:             biased;
90:             _ = cancel.cancelled() => Err(AppError::Timeout("Knowledge indexing was canceled".into())),
91:             result = self.documents.replace(scope,&prepared) => result,
92:         }
93:     }
94:     pub async fn shutdown(&self) -> AppResult<()> {
95:         {
96:             let _registration = self.registration.lock().map_err(|_| {
97:                 AppError::ServiceUnavailable("Knowledge indexing shutdown is unavailable".into())
98:             })?;
99:             self.stop.cancel();
100:             self.slots.close();
101:             self.admission.close();
102:             self.tasks.close();
103:         }
104:         tokio::time::timeout(Duration::from_secs(5), self.tasks.wait())
105:             .await
106:             .map_err(|_| {
107:                 AppError::Timeout("Knowledge indexing did not stop within five seconds".into())
108:             })
109:     }
```

## src/adapters/embedding/mod.rs

Whole-file SHA256: 2e17ba619cb9014bb84a94435c138436f425c84151ecc702aa41e569ce94bca9

Lines 86–117:
```text
86:     fn tokens(&self, input: &str) -> AppResult<usize> {
87:         self.counter
88:             .encode(input, true)
89:             .map(|encoding| encoding.len())
90:             .map_err(|_| unavailable())
91:     }
92:     fn embed(&self, inputs: Vec<String>) -> AppResult<Vec<[f32; 384]>> {
93:         if inputs.is_empty() || inputs.len() > EMBEDDING_BATCH_SIZE {
94:             return Err(AppError::BadRequest(
95:                 "The embedding batch exceeds the local limit".into(),
96:             ));
97:         }
98:         for input in &inputs {
99:             if self.tokens(input)? > MAX_EMBEDDING_TOKENS {
100:                 return Err(AppError::BadRequest(
101:                     "The text exceeds the local model token limit".into(),
102:                 ));
103:             }
104:         }
105:         let count = inputs.len();
106:         let mut model = self.model.lock().map_err(|_| unavailable())?;
107:         let vectors = model
108:             .embed(inputs, Some(EMBEDDING_BATCH_SIZE))
109:             .map_err(|_| unavailable())?;
110:         if vectors.len() != count {
111:             return Err(unavailable());
112:         }
113:         vectors
114:             .into_iter()
115:             .map(|vector| normalize_embedding(vector.try_into().map_err(|_| unavailable())?))
116:             .collect()
117:     }
```

Lines 142–146:
```text
142:     fn embed_query(&self, query: &str) -> AppResult<[f32; 384]> {
143:         self.embed(vec![format!("query: {query}")])?
144:             .pop()
145:             .ok_or_else(unavailable)
146:     }
```

## src/adapters/persistence/knowledge/mod.rs

Whole-file SHA256: b8f7f719fa21d98b86b09af9e66c6777185ecb7c2f2ef683679e38bd17957690

Lines 95–123:
```text
95:     async fn replace(
96:         &self,
97:         scope: KnowledgeScope,
98:         documents: &[PreparedKnowledgeDocument],
99:     ) -> AppResult<Vec<KnowledgeDocumentLayer>> {
100:         let sent = AtomicBool::new(false);
101:         bounded(&sent, async {
102:             let mut tx = authority::start(self.pool(), team_scope(scope)).await?;
103:             let result = async {
104:                 let actor = authorize(&mut tx, scope).await?;
105:                 let layers: Vec<_> = documents
106:                     .iter()
107:                     .map(|document| document.file.size())
108:                     .collect();
109:                 authorize_layers(actor, &layers)?;
110:                 writes::validate(documents)?;
111:                 rows::project_quota(&mut tx, scope, &layers).await?;
112:                 let mut ids = Vec::with_capacity(documents.len());
113:                 for document in documents {
114:                     ids.push(writes::replace(&mut tx, scope, document).await?);
115:                 }
116:                 rows::check_quota(&mut tx, scope).await?;
117:                 rows::by_ids(&mut tx, scope, &ids).await
118:             }
119:             .await;
120:             finish(tx, result, &sent).await
121:         })
122:         .await
123:     }
```

## src/adapters/persistence/knowledge/rows.rs

Whole-file SHA256: be28cea7dd53fe0f0dee0f27e294ed09d5800507fe4a0db08a8ad4c2a0f42535

Lines 102–149:
```text
102: pub(super) async fn project_quota(
103:     tx: &mut Transaction<'_, Postgres>,
104:     scope: KnowledgeScope,
105:     layers: &[KnowledgeLayerSize],
106: ) -> AppResult<()> {
107:     let query = format!("{SELECT} WHERE document.company_id=$1 LIMIT 4097");
108:     let current = decode(
109:         sqlx::query_as::<_, LayerRow>(&query)
110:             .bind(scope.company.as_uuid())
111:             .fetch_all(&mut **tx)
112:             .await
113:             .map_err(authority::database)?,
114:     )?;
115:     let mut projected: HashMap<(KnowledgeDocumentName, AccessLevel), u64> = current
116:         .into_iter()
117:         .map(|row| ((row.name, row.access_level), row.text_bytes))
118:         .collect();
119:     for layer in layers {
120:         if layer.text_bytes > MAX_KNOWLEDGE_FILE_BYTES as u64 {
121:             return Err(quota());
122:         }
123:         projected.insert((layer.name.clone(), layer.access_level), layer.text_bytes);
124:     }
125:     if projected.len() > MAX_COMPANY_KNOWLEDGE_LAYERS
126:         || projected.values().sum::<u64>() > MAX_COMPANY_KNOWLEDGE_BYTES as u64
127:     {
128:         return Err(quota());
129:     }
130:     Ok(())
131: }
132: #[derive(sqlx::FromRow)]
133: struct Usage {
134:     layers: i64,
135:     bytes: i64,
136: }
137: pub(super) async fn check_quota(
138:     tx: &mut Transaction<'_, Postgres>,
139:     scope: KnowledgeScope,
140: ) -> AppResult<()> {
141:     let usage=sqlx::query_as::<_,Usage>("SELECT count(*) AS layers,coalesce(sum(octet_length(content)),0)::bigint AS bytes FROM knowledge_documents WHERE company_id=$1")
142:         .bind(scope.company.as_uuid()).fetch_one(&mut **tx).await.map_err(authority::database)?;
143:     if usage.layers > MAX_COMPANY_KNOWLEDGE_LAYERS as i64
144:         || usage.bytes > MAX_COMPANY_KNOWLEDGE_BYTES as i64
145:     {
146:         return Err(quota());
147:     }
148:     Ok(())
149: }
```

## src/application/workflow/publication/types.rs

Whole-file SHA256: ead7529b8238b82e1ebb9dcfab4c8c38782cb5b9a25bdfb171b24f47b0373256

Lines 120–155:
```text
120: pub enum ToolKind<'a> {
121:     /// A trusted built-in `message.*` tool.
122:     Message,
123:     /// A trusted built-in provisioning tool (p08 D2.1). Only a name-only snapshot carries a
124:     /// provisioning name this way; an MCP tool of the same name stays a slot tool.
125:     Provisioning,
126:     /// A tool reached through one of the workflow's resource slots (MCP or slot-bound HTTP).
127:     Slot(&'a ResourceName),
128:     /// A company HTTP operation registered by name.
129:     Http,
130:     /// A pinned child workflow registered by name.
131:     Workflow(&'a WorkflowToolPin),
132: }
133: 
134: impl ToolSnapshot {
135:     pub fn kind(&self) -> ToolKind<'_> {
136:         if let Some(pin) = &self.workflow {
137:             return ToolKind::Workflow(pin);
138:         }
139:         if crate::application::workflow::message::is_builtin_message_tool(
140:             self.contract.name.as_str(),
141:         ) {
142:             return ToolKind::Message;
143:         }
144:         if self.contract.connection.is_none()
145:             && crate::application::workflow::provisioning::is_builtin_provisioning_tool(
146:                 self.contract.name.as_str(),
147:             )
148:         {
149:             return ToolKind::Provisioning;
150:         }
151:         match &self.contract.connection {
152:             Some(slot) => ToolKind::Slot(slot),
153:             None => ToolKind::Http,
154:         }
155:     }
```

## src/application/workflow/agent.rs

Whole-file SHA256: b3a0cab5c7e122545bdd886390adafa63f792d806a337994a174ee386836d400

Lines 259–279:
```text
259: fn prepared_tool(snapshot: &ToolSnapshot, target: AgentToolTarget) -> AppResult<PreparedTool> {
260:     let name = snapshot.contract.name.clone();
261:     let consistent = match (&target, snapshot.kind()) {
262:         (AgentToolTarget::Message { name: target }, ToolKind::Message) => target == &name,
263:         (AgentToolTarget::Provisioning { name: target }, ToolKind::Provisioning) => target == &name,
264:         (AgentToolTarget::Mcp { slot }, ToolKind::Slot(frozen)) => frozen == slot,
265:         (AgentToolTarget::Http { .. }, ToolKind::Http) => true,
266:         (AgentToolTarget::Workflow { pin }, ToolKind::Workflow(frozen)) => frozen == pin,
267:         _ => false,
268:     };
269:     let declared = name.as_str().replace('.', "__");
270:     if !consistent || declared.len() > MAX_DECLARED_NAME {
271:         return Err(invalid("Agent tool has no supported dispatch binding"));
272:     }
273:     Ok(PreparedTool {
274:         declared,
275:         name,
276:         input_schema: snapshot.contract.input_schema.clone(),
277:         target,
278:     })
279: }
```

## src/application/workflow/sample/tools.rs

Whole-file SHA256: 24a71d4f80c87fcb56f8fd1d297aee4ad9081a8c75f6882b478ae7a41e042fbe

Lines 59–113:
```text
59:     pub(super) fn tool(
60:         &mut self,
61:         bundle: &Arc<PublishedBundle>,
62:         frame: &Frame,
63:         path: &SamplePath,
64:         tool: &ToolSnapshot,
65:         input: Value,
66:         fixture: Option<Value>,
67:     ) -> AppResult<Value> {
68:         validate(&tool.contract.input_schema, &input)?;
69:         let (output, kind) = match tool.kind() {
70:             ToolKind::Workflow(pin) => {
71:                 if fixture.is_some() {
72:                     return Err(refused(
73:                         "Workflow tool output comes from its sample child, not a fixture",
74:                     ));
75:                 }
76:                 let child = bundle
77:                     .children()
78:                     .get(&pin.version_id)
79:                     .ok_or_else(|| refused("Workflow tool child is not frozen"))?
80:                     .clone();
81:                 let output = self.run(
82:                     child,
83:                     frame.child(
84:                         path.child("workflow-tool"),
85:                         input.clone(),
86:                         pin.params.clone(),
87:                     ),
88:                 )?;
89:                 (output, SampleEffectKind::WorkflowChild)
90:             }
91:             ToolKind::Message | ToolKind::Provisioning | ToolKind::Slot(_) | ToolKind::Http => {
92:                 let output = fixture.ok_or_else(|| {
93:                     refused("External tool needs a fixture output; no production fallback exists")
94:                 })?;
95:                 (output, super::dispatch::effect_kind(tool.policy.effect))
96:             }
97:         };
98:         let schema = match tool.kind() {
99:             ToolKind::Slot(_) => {
100:                 super::super::registry::mcp_tool_result(&tool.contract.output_schema)
101:             }
102:             _ => tool.contract.output_schema.clone(),
103:         };
104:         validate(&schema, &output)?;
105:         self.effect(SampleEffect {
106:             path: path.clone(),
107:             operation: tool.contract.name.clone(),
108:             kind,
109:             input,
110:             output: output.clone(),
111:         })?;
112:         Ok(output)
113:     }
```

## migrations/20260817000000_init_schema.sql

Whole-file SHA256: 07516bb77778bcc9cbf7d58730179a3e404e95586ee6c5bd79248f4497d19dd9

Lines 11774–11829:
```text
11774: CREATE TABLE public.workflow_history_consumptions (
11775:     company_id uuid NOT NULL,
11776:     run_id uuid NOT NULL,
11777:     execution_id uuid NOT NULL,
11778:     job_id uuid NOT NULL,
11779:     attempt_number integer NOT NULL CHECK (attempt_number > 0),
11780:     execution_generation uuid NOT NULL,
11781:     worker_id uuid NOT NULL,
11782:     content_level public.access_level NOT NULL,
11783:     PRIMARY KEY (company_id,run_id,execution_id,job_id,attempt_number),
11784:     FOREIGN KEY (company_id,run_id,execution_id)
11785:         REFERENCES public.workflow_executions(company_id,run_id,id) ON DELETE CASCADE,
11786:     FOREIGN KEY (company_id,execution_id,job_id)
11787:         REFERENCES public.background_tasks(company_id,workflow_execution_id,id) ON DELETE CASCADE,
11788:     FOREIGN KEY (job_id,attempt_number,execution_generation,worker_id)
11789:         REFERENCES public.task_attempts(task_id,attempt_number,execution_generation,worker_id) ON DELETE CASCADE
11790: );
11791: 
11792: CREATE FUNCTION public.guard_workflow_history_consumption_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
11793: BEGIN
11794:     IF TG_OP='UPDATE' AND NEW IS NOT DISTINCT FROM OLD THEN RETURN NEW; END IF;
11795:     IF TG_OP='DELETE' AND pg_trigger_depth()>1 AND (
11796:         NOT EXISTS (SELECT 1 FROM public.workflow_runs WHERE company_id=OLD.company_id AND id=OLD.run_id)
11797:         OR NOT EXISTS (SELECT 1 FROM public.background_tasks WHERE company_id=OLD.company_id AND id=OLD.job_id)
11798:         OR NOT EXISTS (SELECT 1 FROM public.task_attempts WHERE task_id=OLD.job_id AND attempt_number=OLD.attempt_number)
11799:     ) THEN RETURN OLD; END IF;
11800:     RAISE EXCEPTION 'workflow history consumption proof is immutable'
11801:         USING ERRCODE='23514', CONSTRAINT='workflow_guard:workflow_history_consumption_immutable';
11802: END $$;
11803: CREATE TRIGGER workflow_history_consumption_immutable BEFORE UPDATE OR DELETE ON public.workflow_history_consumptions
11804:     FOR EACH ROW EXECUTE FUNCTION public.guard_workflow_history_consumption_immutable();
11805: 
11806: CREATE FUNCTION public.guard_workflow_history_consumption_live() RETURNS trigger LANGUAGE plpgsql AS $$
11807: BEGIN
11808:     IF NOT EXISTS (
11809:         SELECT 1 FROM public.background_tasks AS job
11810:         JOIN public.workflow_executions AS execution ON execution.company_id=job.company_id
11811:             AND execution.id=job.workflow_execution_id
11812:         JOIN public.workflow_runs AS run ON run.company_id=execution.company_id AND run.id=execution.run_id
11813:         JOIN public.task_attempts AS attempt ON attempt.task_id=job.id
11814:             AND attempt.attempt_number=NEW.attempt_number AND attempt.execution_generation=NEW.execution_generation
11815:             AND attempt.worker_id=NEW.worker_id AND attempt.status='processing'
11816:         WHERE job.company_id=NEW.company_id AND job.id=NEW.job_id AND execution.id=NEW.execution_id AND run.id=NEW.run_id
11817:             AND job.status='processing' AND job.worker_id=NEW.worker_id
11818:             AND job.execution_generation=NEW.execution_generation AND job.attempt_ordinal=NEW.attempt_number
11819:             AND job.lock_expires_at>clock_timestamp() AND execution.completed_at IS NULL
11820:             AND run.state IN ('queued','running') AND run.deadline>clock_timestamp()
11821:             AND run.content_level>=NEW.content_level
11822:     ) THEN
11823:         RAISE EXCEPTION 'workflow history consumption lost live fence'
11824:             USING ERRCODE='23514', CONSTRAINT='workflow_guard:workflow_history_consumption_live';
11825:     END IF;
11826:     RETURN NEW;
11827: END $$;
11828: CREATE CONSTRAINT TRIGGER workflow_history_consumption_live AFTER INSERT OR UPDATE ON public.workflow_history_consumptions
11829:     DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.guard_workflow_history_consumption_live();
```

