# CH-P01-AG3-R2 affected-contract inspection

Architecture correction inspection only; no implementation/runtime test claim. Reuses N1 source excerpts for unchanged admission, native observation, run-first lease/dispatch, historical restore and retention contracts.

## /private/tmp/openbot-cloudroom-harness-t1gfnhz1/cloudroom/gui/packages/host-daemon-contract/src/session.ts:783
```
783: export const hostDaemonToolCallRequestSchema = toolCallRequestSchema
784:   .pick({
785:     threadId: true,
786:     providerThreadId: true,
787:     turnId: true,
788:     callId: true,
789:     tool: true,
790:     arguments: true,
791:   })
792:   .extend({
793:     sessionId: z.string().min(1),
794:   });
795: export type HostDaemonToolCallRequest = z.infer<
796:   typeof hostDaemonToolCallRequestSchema
797: >;
```

## /private/tmp/openbot-cloudroom-harness-t1gfnhz1/cloudroom/gui/apps/host-daemon/src/server-client.ts:566
```
566:     async callTool(
567:       request: ToolCallRequest,
568:       signal?: AbortSignal,
569:     ): Promise<HostDaemonToolCallResponse> {
570:       const payload: HostDaemonToolCallRequest = {
571:         threadId: request.threadId,
572:         providerThreadId: request.providerThreadId,
573:         turnId: request.turnId,
574:         callId: request.callId,
575:         tool: request.tool,
576:         ...(request.arguments !== undefined
577:           ? { arguments: request.arguments }
578:           : {}),
579:         sessionId: requireSessionId(),
580:       };
581:       const response = await fetchFn(buildInternalUrl("/session/tool-call"), {
582:         signal,
583:         method: "POST",
584:         headers: headers(),
585:         body: JSON.stringify(payload),
586:       });
587: 
588:       if (!response.ok) {
589:         throw await createResponseError("call tool", response);
590:       }
591: 
592:       return hostDaemonToolCallResponseSchema.parse(await response.json());
```

## /private/tmp/openbot-cloudroom-harness-t1gfnhz1/cloudroom/gui/apps/host-daemon/src/app.ts:550
```
550:     onToolCall: async (request, signal) => {
551:       try {
552:         await flushThreadEvents();
553:         signal?.throwIfAborted();
554:         return await runSessionRequest({
555:           source: "callTool",
556:           request: () => serverClient.callTool(request, signal),
557:         });
558:       } catch (error) {
```

## /private/tmp/openbot-cloudroom-harness-t1gfnhz1/cloudroom/gui/apps/server/src/internal/tool-calls.ts:22
```
22: 
23: function streamToolCallResponse(
24:   result: Promise<ToolCallResponse>,
25:   abortController: AbortController,
26: ): Response {
27:   const body = new ReadableStream<Uint8Array>({
28:     cancel() {
29:       abortController.abort();
30:     },
31:     start(controller) {
32:       void result.then(
33:         (response) => {
34:           try {
35:             controller.enqueue(textEncoder.encode(JSON.stringify(response)));
36:             controller.close();
37:           } catch (error) {
38:             controller.error(error);
39:           }
40:         },
41:         (error) => controller.error(error),
42:       );
43:     },
44:   });
45:   return new Response(body, {
46:     headers: { "content-type": "application/json; charset=UTF-8" },
```

## /private/tmp/openbot-cloudroom-harness-t1gfnhz1/cloudroom/gui/apps/server/src/internal/tool-calls.ts:56
```
56:     "/session/tool-call",
57:     hostDaemonToolCallRequestSchema,
58:     async (context, payload) => {
59:       const session = requireAuthenticatedDaemonSession({
60:         context,
61:         db: deps.db,
62:         sessionId: payload.sessionId,
63:       });
64:       const { environment, thread } = requireThreadEnvironment(
65:         deps.db,
66:         payload.threadId,
67:       );
68:       if (environment.hostId !== session.hostId) {
69:         throw new ApiError(
70:           403,
71:           "invalid_request",
72:           "Thread does not belong to the session host",
73:         );
74:       }
75: 
76:       if (payload.tool === UPDATE_ENVIRONMENT_DIRECTORY_TOOL_NAME) {
77:         return context.json(
78:           await handleUpdateEnvironmentDirectoryToolCall(deps, {
79:             currentEnvironment: environment,
80:             input: payload.arguments,
81:             thread,
82:             turnId: payload.turnId,
83:           }),
84:         );
85:       }
86: 
87:       const pluginTool = findPluginAgentTool(payload.tool);
88:       if (pluginTool) {
89:         const controller = new AbortController();
90:         const signal = AbortSignal.any([
91:           context.req.raw.signal,
92:           controller.signal,
93:         ]);
94:         return streamToolCallResponse(
95:           invokePluginAgentTool(pluginTool, {
96:             input: payload.arguments,
97:             ctx: {
98:               threadId: thread.id,
99:               projectId: thread.projectId,
100:               signal,
101:             },
102:           }),
103:           controller,
104:         );
105:       }
106: 
107:       return context.json({
108:         success: false,
109:         contentItems: [
110:           { type: "inputText", text: `Unsupported tool: ${payload.tool}` },
111:         ],
112:       });
113:     },
114:   );
115: }
```

## /private/tmp/openbot-cloudroom-harness-t1gfnhz1/cloudroom/gui/packages/agent-runtime/src/bridge-protocol-adapter.ts:585
```
585:       request: ProviderInboundRequest,
586:     ): DecodedToolCallRequest | null {
587:       if (typeof request.id !== "string" && typeof request.id !== "number") {
588:         return null;
589:       }
590:       const decoded = decodeNormalizedProviderToolCallRequest(
591:         request.id,
592:         request.method,
593:         request.params,
594:       );
595:       if (decoded === null) {
596:         return decoded;
597:       }
598:       const marker = providerNativeIdsParamsSchema.safeParse(request.params);
599:       if (
600:         marker.success !== true ||
601:         marker.data.providerNativeIds !== true ||
602:         decoded.threadId === undefined
603:       ) {
604:         return decoded;
605:       }
606:       return {
607:         ...decoded,
608:         turnId:
609:           decoded.turnId === null
610:             ? null
611:             : (deltaAssembler.getBbTurnId(decoded.threadId, decoded.turnId) ??
612:               decoded.turnId),
613:         callId:
614:           deltaAssembler.getBbItemId(decoded.threadId, decoded.callId) ??
615:           decoded.callId,
616:       };
617:     },
```

## integrations/cloudroom-plugin/src/turns.ts:39
```
39: export function applyNativeEvents(original: CommandRecord, events: readonly Event[]): TurnUpdate {
40:   if (events.length > LIMITS.page || !original.threadId) throw new Error('native event page bound');
41:   const record = structuredClone(original);
42:   if (record.correlationConflict) return { kind: 'uncertain', record };
43:   const conflict = (): TurnUpdate => {
44:     record.correlationConflict = true; record.stage = 'uncertain';
45:     return { kind: 'uncertain', record };
46:   };
47:   for (const event of events) {
48:     if (!Number.isSafeInteger(event.seq) || event.seq <= record.cursor) throw new Error('native event cursor did not advance');
49:     // Cloudroom prunes resolved streaming deltas without renumbering retained
50:     // events. A gap is not a lost request/turn binding; expose incomplete history
51:     // conservatively while still requiring every positive correlation below.
52:     if (event.seq > record.cursor + 1) record.truncated = true;
53:     record.cursor = event.seq;
54:     if (event.type === 'client/turn/requested' && ownInput(event.data.input, record.sdkInput)
55:         && event.data.source === (record.operation.command.kind === 'create' ? 'spawn' : 'tell')) {
56:       if (record.requestId && record.requestId !== event.data.requestId) return conflict();
57:       record.requestId = event.data.requestId;
58:     }
59:     if (event.type === 'turn/input/accepted' && record.requestId !== null && event.data.clientRequestId === record.requestId) {
60:       if (event.scope.kind !== 'turn' || (record.turnId && record.turnId !== event.scope.turnId)) return conflict();
61:       record.turnId = event.scope.turnId; record.nativeState = 'running'; record.stage = 'observing';
62:     }
63:     if (event.scope.kind !== 'turn' || event.scope.turnId !== record.turnId) continue;
64:     if (event.type.startsWith('item/') && record.nativeState === 'waiting_for_input') record.nativeState = 'running';
65:     if (event.type === 'item/completed'
66:         && event.data.item.type === 'agentMessage' && !event.data.item.parentToolCallId) {
67:       replaceText(record, event.data.item.id, event.data.item.text);
68:     }
69:     if (event.type === 'turn/completed') {
70:       record.nativeState = event.data.status === 'completed' ? 'completed' : event.data.status === 'interrupted' ? 'stopped' : 'failed';
71:       record.stage = 'terminal';
72:       return { kind: 'terminal', record, evidence: evidence(record) };
73:     }
74:   }
75:   const observed = evidence(record);
76:   const prior = evidence(original);
77:   return { kind: JSON.stringify(observed) === JSON.stringify(prior) ? 'unchanged' : 'progress', record, evidence: observed };
78: }
```

## integrations/cloudroom-plugin/src/journal.ts:280
```
280:   acknowledge(through: number): void {
281:     this.db.transaction(() => {
282:       const meta = this.meta();
283:       if (!Number.isSafeInteger(through) || through < meta.acknowledged || through > meta.allocated) throw new RecoveryRequired();
284:       this.db.prepare('UPDATE journal_meta SET acknowledged=? WHERE id=1').run(through);
285:       this.db.prepare("UPDATE commands SET acknowledged_at=COALESCE(acknowledged_at,?) WHERE terminal=1 AND acknowledged_at IS NULL AND COALESCE(json_extract(state,'$.lastObservation.sequence'),json_extract(state,'$.lastObservation.observation.sequence'))<=?").run(Date.now(), through);
286:       this.db.prepare('DELETE FROM outbox WHERE sequence<=?').run(through);
287:     })();
288:   }
```

## src/adapters/persistence/workflow/action_reconciliation/settlement.rs:174
```
174: async fn continue_on(
175:     tx: &mut Transaction<'_, Postgres>,
176:     command: &ReconcileActionCommand,
177:     head: &RunHead,
178:     outcome: ReconciliationOutcome,
179: ) -> AppResult<(ReconciliationOutcome, Option<Uuid>)> {
180:     if !matches!(
181:         outcome,
182:         ReconciliationOutcome::AppliedRecorded { receipt: true }
183:             | ReconciliationOutcome::NotAppliedRecorded
184:     ) {
185:         return Ok((outcome, None));
186:     }
187:     if head.state != RunState::Waiting(WaitingReason::Reconciliation) {
188:         return Ok((outcome, None));
189:     }
190:     let job: Option<Uuid> = sqlx::query_scalar("SELECT id FROM background_tasks WHERE company_id=$1 AND workflow_execution_id=$2  AND status='failed'")
191:         .bind(command.scope.company.as_uuid()).bind(command.scope.execution.as_uuid()).fetch_optional(&mut **tx).await?;
192:     let Some(job) = job else {
193:         return Ok((
194:             ReconciliationOutcome::Blocked {
195:                 reason: ReconciliationBlockedReason::IneligibleJob,
196:             },
197:             None,
198:         ));
199:     };
200:     let eligible: bool =
201:         sqlx::query_scalar("SELECT workflow_action_reconciliation_reopen_safe($1,$2,$3)")
202:             .bind(command.scope.company.as_uuid())
203:             .bind(command.scope.run.as_uuid())
204:             .bind(job)
205:             .fetch_one(&mut **tx)
206:             .await?;
207:     if !eligible {
208:         return Ok((
209:             ReconciliationOutcome::Blocked {
210:                 reason: ReconciliationBlockedReason::IneligibleJob,
211:             },
212:             None,
213:         ));
214:     }
215:     let receipt_only: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM workflow_action_dispatches AS marker WHERE marker.company_id=$1 AND marker.execution_id=$2 AND NOT EXISTS(SELECT 1 FROM workflow_action_receipts AS receipt WHERE receipt.company_id=marker.company_id AND receipt.invocation_id=marker.invocation_id))")
216:         .bind(command.scope.company.as_uuid()).bind(command.scope.execution.as_uuid()).fetch_one(&mut **tx).await?;
217:     sqlx::query("UPDATE background_tasks SET status='pending',run_at=clock_timestamp(),updated_at=clock_timestamp() WHERE company_id=$1 AND id=$2")
218:         .bind(command.scope.company.as_uuid()).bind(job).execute(&mut **tx).await?;
219:     sqlx::query("UPDATE workflow_runs SET state='running',waiting_reason=NULL WHERE company_id=$1 AND id=$2")
220:         .bind(command.scope.company.as_uuid()).bind(command.scope.run.as_uuid()).execute(&mut **tx).await?;
221:     Ok((ReconciliationOutcome::Scheduled { receipt_only }, Some(job)))
222: }
```
