# CH-P01-AG3-R2 producer follow-up before return

Root supplied the producer path after the initial N2 records were saved, before any PASS was returned. B independently inspected the full producer-to-runtime fallback below. This supersedes the initial identity conclusion; source remains unchanged.

## gui/packages/provider-bridge-protocol/src/bridge-kit/pending-tool-call-tracker.ts:16
```
16: interface PendingToolCall {
17:   resolve: (value: BridgeToolCallResult) => void;
18:   scope: object;
19: }
20: 
21: export interface ForwardBridgeToolCallArgs {
22:   arguments: Record<string, unknown>;
23:   providerThreadId: string;
24:   scope: object;
25:   threadId: string;
26:   toolName: string;
27: }
28: 
29: export interface PendingToolCallTracker {
30:   forwardToolCall: (
31:     args: ForwardBridgeToolCallArgs,
32:   ) => Promise<BridgeToolCallResult>;
33:   handleToolCallResponse: (response: BridgeJsonRpcResponse) => boolean;
34:   resolvePendingToolCalls: (scope: object, message: string) => void;
35: }
36: 
37: export function createPendingToolCallTracker(options: {
38:   sendToolCall: (request: BridgeToolCallRequest) => void;
39: }): PendingToolCallTracker {
40:   const pendingToolCalls = new Map<string | number, PendingToolCall>();
41:   let requestIdCounter = 0;
42: 
43:   return {
44:     forwardToolCall: (args) => {
45:       return new Promise<BridgeToolCallResult>((resolve) => {
46:         requestIdCounter += 1;
47:         const requestId = requestIdCounter;
48:         pendingToolCalls.set(requestId, { resolve, scope: args.scope });
49:         try {
50:           options.sendToolCall({
51:             jsonrpc: "2.0",
52:             id: requestId,
53:             method: "item/tool/call",
54:             params: {
55:               threadId: args.threadId,
56:               providerThreadId: args.providerThreadId,
57:               turnId: null,
58:               callId: `call-${requestId}`,
59:               tool: args.toolName,
60:               arguments: args.arguments,
61:             },
62:           });
63:         } catch (error) {
64:           pendingToolCalls.delete(requestId);
65:           resolve({
66:             content: error instanceof Error ? error.message : String(error),
67:             isError: true,
68:           });
```

## gui/plugins/provider-claude-code/src/bridge/bridge.ts:474
```
474: function createForwardToolCall(getThreadId: () => string): ToolCallForwarder {
475:   return (toolName, args) => {
476:     const threadId = getThreadId();
477:     const attachment = threadAttachments.get(threadId);
478:     const threadSession = attachment?.residentSession;
479:     if (
480:       !attachment ||
481:       attachment.closing ||
482:       !threadSession ||
483:       threadSession.closing
484:     ) {
485:       return Promise.resolve({
486:         content: "Thread session not found",
487:         isError: true,
488:       });
489:     }
490:     return forwardToolCall({
491:       arguments: args,
492:       providerThreadId: attachment.providerThreadId ?? threadId,
493:       scope: threadSession,
494:       threadId,
495:       toolName,
496:     });
497:   };
498: }
499: 
```

## gui/plugins/provider-claude-code/src/bridge/tool-proxy-mcp.ts:42
```
42:   }));
43:   instance.server.setRequestHandler(CallToolRequestSchema, async (request) => {
44:     const def = toolsByName.get(request.params.name);
45:     if (def === undefined) {
46:       return {
47:         content: [
48:           {
49:             type: "text" as const,
50:             text: `Unknown tool: ${request.params.name}`,
51:           },
52:         ],
53:         isError: true,
54:       };
55:     }
56:     const result = await forwardToolCall(
57:       def.name,
58:       request.params.arguments ?? {},
59:     );
60:     return {
61:       content: experimental_buildBridgeToolCallContent(result),
```

## gui/packages/agent-runtime/src/runtime-provider-requests.ts:153
```
153: 
154: function resolveRuntimeProviderRequestTurnId(
155:   args: ResolveRuntimeProviderRequestTurnIdArgs,
156: ): string | null {
157:   if (args.turnId !== null) {
158:     return args.turnId;
159:   }
160: 
161:   const activeTurnId = args.getActiveTurnId(args.resolvedThreadId);
162:   if (activeTurnId !== null) {
163:     return activeTurnId;
164:   }
165: 
166:   sendJsonRpcError({
167:     child: args.providerProcess.child,
168:     id: args.parsedId,
169:     message: `Cannot route provider ${args.requestKind} for thread "${args.resolvedThreadId}" without a turn id because no active turn is known`,
170:   });
171:   return null;
```

## gui/packages/agent-runtime/src/runtime-provider-requests.ts:174
```
174: function handleToolCallProviderRequest(
175:   args: HandleRuntimeProviderRequestArgs,
176: ): boolean {
177:   const toolCallReq = args.providerProcess.adapter.decodeToolCallRequest(
178:     args.rawRequest,
179:   );
180:   if (!toolCallReq) {
181:     return false;
182:   }
183: 
184:   const resolvedThreadId = args.resolveThreadId({
185:     parsedId: args.parsedId,
186:     providerThreadId: toolCallReq.providerThreadId,
187:     requestKind: "tool call",
188:     threadIdHint: toolCallReq.threadId,
189:   });
190:   if (!resolvedThreadId) {
191:     return true;
192:   }
193:   const resolvedTurnId = resolveRuntimeProviderRequestTurnId({
194:     ...args,
195:     requestKind: "tool call",
196:     resolvedThreadId,
197:     turnId: toolCallReq.turnId,
198:   });
199:   if (resolvedTurnId === null) {
200:     return true;
201:   }
202: 
203:   const scopedToolCallReq: ToolCallRequest = {
204:     requestId: toolCallReq.requestId,
205:     threadId: resolvedThreadId,
206:     providerThreadId: toolCallReq.providerThreadId,
207:     turnId: resolvedTurnId,
208:     callId: toolCallReq.callId,
209:     tool: toolCallReq.tool,
210:     ...(toolCallReq.arguments !== undefined
211:       ? { arguments: toolCallReq.arguments }
212:       : {}),
213:   };
214:   const scope = args.providerProcess.interactiveRequestScope;
```
