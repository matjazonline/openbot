# CH-P01-AG2 pinned-source inspection

Cloudroom HEAD d8b3cc3a16bb4b06fc99d8e543d40f1087d1e815. Source inspection, not execution evidence.

## gui/apps/server/src/services/threads/thread-create.ts:725-768

SHA256 `3b70dd43bd7df639a4bb7737ee340d0f89fe6c822cfd61a6d50cf085cea353fc`

```text
725:   const originKind = requestInput.originKind ?? null;
726:   const sourceThreadId =
727:     requestInput.sourceThreadId ??
728:     (originKind !== null ? requestInput.parentThreadId : undefined);
729:   const hierarchyParentThreadId =
730:     originKind === null ? requestInput.parentThreadId : undefined;
731:   const parentThread = hierarchyParentThreadId
732:     ? assertValidParentThread(deps, {
733:         parentThreadId: hierarchyParentThreadId,
734:       })
735:     : null;
736:   if (originKind === null && sourceThreadId !== undefined) {
737:     throw new ApiError(
738:       400,
739:       "invalid_request",
740:       "sourceThreadId requires an originKind",
741:     );
742:   }
743:   if (originKind === null && requestInput.sourceSeqEnd !== undefined) {
744:     throw new ApiError(
745:       400,
746:       "invalid_request",
747:       "sourceSeqEnd requires an originKind",
748:     );
749:   }
750:   const sourceThread = sourceThreadId
751:     ? requireLiveSourceThread(deps, {
752:         projectId: requestInput.projectId,
753:         sourceThreadId,
754:       })
755:     : null;
756:   if (originKind !== null && sourceThread !== null) {
757:     assertValidParentThread(deps, {
758:       parentThreadId: sourceThread.id,
759:       cloudChild: options.forkPoint !== undefined || options.importedText !== undefined,
760:     });
761:   }
762:   if (originKind !== null && sourceThread === null) {
763:     throw new ApiError(
764:       400,
765:       "invalid_request",
766:       "originKind requires a sourceThreadId",
767:     );
768:   }
```

## gui/apps/server/src/services/threads/thread-create.ts:945-965

SHA256 `3b70dd43bd7df639a4bb7737ee340d0f89fe6c822cfd61a6d50cf085cea353fc`

```text
945: 
946:   const fork = options.freshFork
947:     ? null
948:     : (options.importedFork ??
949:     options.forkPoint ??
950:     (options.importedText ? null : resolveForkPoint(deps, {
951:       originKind: request.originKind ?? null,
952:       providerId: request.providerId,
953:       sourceSeqEnd: request.sourceSeqEnd,
954:       sourceThread,
955:     })));
956: 
957:   if (request.originKind !== null && fork === null && !options.importedText && !options.freshFork) {
958:     throw new ApiError(
959:       400,
960:       "fork_source_session_unavailable",
961:       "Cannot fork: source has no active session to clone",
962:     );
963:   }
964: 
965:   const createArgs = {
```

## gui/apps/server/src/services/threads/thread-provisioning.ts:258-267

SHA256 `c1e3e5ff35f741992bb32ffea19af5fcd3cc6f9b5950cf411539c536f7074212`

```text
258:     const context = createThreadStartup({
259:       ...args,
260:       clientRequestId: request.requestId,
261:       input: args.providerInput ?? args.input,
262:       seedWithoutRun:
263:         args.seedWithoutRun === true ||
264:         (args.startedOnBehalfOf !== null && args.thread.originKind !== null),
265:     });
266:     saveThreadProvisionContext({
267:       replace: true,
```

## gui/apps/server/src/services/threads/thread-parent.ts:22-28

SHA256 `d45e47df3b9d226301095fb8766d50a32484cf4f3ece3d704a5840840f1efc1e`

```text
22: 
23: export function isParentNotifiableChildThread<
24:   T extends Pick<Thread, "parentThreadId" | "originKind">,
25: >(thread: T): thread is T & { parentThreadId: string } {
26:   return isAgentDelegatedChildThread(thread) && thread.originKind === null;
27: }
28: 
```

## gui/apps/server/src/services/skills/injected-skills.ts:227-286

SHA256 `6fae31d6792248e2815ff04a1b0dcad2e59cfa04e7c3d4bac9a1460ea3710bc9`

```text
227: function collectSkillTreeEntries(
228:   rootPath: string,
229:   currentPath = rootPath,
230: ): SkillTreeEntry[] {
231:   const entries: SkillTreeEntry[] = [];
232:   for (const entry of fs
233:     .readdirSync(currentPath, { withFileTypes: true })
234:     .sort(sortDirentsByName)) {
235:     if (
236:       currentPath === rootPath &&
237:       entry.name === REGISTRY_SKILL_PROVENANCE_FILE_NAME
238:     ) {
239:       continue;
240:     }
241:     const entryPath = path.join(currentPath, entry.name);
242:     const stat = fs.lstatSync(entryPath);
243:     if (stat.isSymbolicLink()) {
244:       throw new Error(`Skill tree contains a symlink: ${entryPath}`);
245:     }
246:     if (stat.isDirectory()) {
247:       entries.push(...collectSkillTreeEntries(rootPath, entryPath));
248:       continue;
249:     }
250:     if (!stat.isFile()) {
251:       throw new Error(`Skill tree entry is not a regular file: ${entryPath}`);
252:     }
253:     entries.push({
254:       bytes: fs.readFileSync(entryPath),
255:       mode: stat.mode & 0o777,
256:       path: normalizeRelativePath(path.relative(rootPath, entryPath)),
257:     });
258:   }
259:   return entries;
260: }
261: 
262: export function hashSkillTreeEntries(
263:   entries: readonly SkillTreeEntry[],
264: ): string {
265:   const hash = createHash("sha256");
266:   hash.update("bb-skill-tree-v1");
267:   for (const entry of [...entries].sort((left, right) =>
268:     compareStringsByCodePoint(left.path, right.path),
269:   )) {
270:     hash.update("\0file\0");
271:     hash.update(entry.path);
272:     hash.update("\0");
273:     hash.update(entry.mode.toString(8));
274:     hash.update("\0");
275:     hash.update(String(entry.bytes.length));
276:     hash.update("\0");
277:     hash.update(entry.bytes);
278:   }
279:   return hash.digest("hex");
280: }
281: 
282: export function readSkillTreeManifest(rootPath: string): SkillTreeManifest {
283:   const entries = collectSkillTreeEntries(rootPath);
284:   return { entries, treeHash: hashSkillTreeEntries(entries) };
285: }
286: 
```

## gui/apps/server/src/internal/skills.ts:16-44

SHA256 `5f8202e7427af4dec359406797ecd9ce995111e142282a43ce87d0de486e6a27`

```text
16:     const requestedHash = context.req.param("hash");
17:     if (!TREE_HASH_PATTERN.test(requestedHash)) {
18:       throw new ApiError(404, "skill_tree_not_found", "Skill tree not found");
19:     }
20:     const sourceRootPath = deps.skillTreeRegistry.resolve(requestedHash);
21:     if (sourceRootPath === undefined) {
22:       throw new ApiError(404, "skill_tree_not_found", "Skill tree not found");
23:     }
24: 
25:     let manifest;
26:     try {
27:       manifest = readSkillTreeManifest(sourceRootPath);
28:     } catch {
29:       throw new ApiError(404, "skill_tree_not_found", "Skill tree not found");
30:     }
31:     if (manifest.treeHash !== requestedHash) {
32:       throw new ApiError(404, "skill_tree_not_found", "Skill tree not found");
33:     }
34: 
35:     return context.json({
36:       treeHash: manifest.treeHash,
37:       entries: manifest.entries.map((entry) => ({
38:         path: entry.path,
39:         mode: entry.mode,
40:         contentBase64: entry.bytes.toString("base64"),
41:       })),
42:     });
43:   });
44: }
```

## gui/apps/server/src/services/skills/registry-skill-provenance.ts:1-8

SHA256 `74786b66d22538fcccdfeb6ddd47dedb7319b75c55d6b09b8f921a8f54966e4f`

```text
1: import { readFileSync } from "node:fs";
2: import fs from "node:fs/promises";
3: import path from "node:path";
4: import { z } from "zod";
5: 
6: export const REGISTRY_SKILL_PROVENANCE_FILE_NAME = ".bb-registry-skill.json";
7: 
8: const registrySkillProvenanceSchema = z
```
