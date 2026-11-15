# CH-P01-AG2-R2 correction inspection

Decision SHA256 08357154aecaa1b044a42688a0f25230eff0e7ca2f9ca56d7bb7989889be9403

Coordination SHA256 360354ed88bb6b64e0b8f3a74ee7facce51c9a7aef44790fd34904042007448d

Pinned Cloudroom HEAD d8b3cc3a16bb4b06fc99d8e543d40f1087d1e815

Read-only source inspection; no execution evidence. Existing collection, store staging, cache, transport union and lineage paths inspected against the proposed corrections. The current cache short-circuit and caught staging errors remain implementation obligations under the proposed exact-hash and required-failure contracts.

## gui/apps/host-daemon/src/injected-skills.ts:537-588

SHA256 `3bdbf39d51b48e0f442bb7d1f9cb8ffa6557d640d55ac88ccadceac8d5e72076`

```text
537: function validatedTreeEntries(tree: HostDaemonSkillTree): CollectedSkillFile[] {
538:   const files: CollectedSkillFile[] = [];
539:   const paths = new Set<string>();
540:   let totalBytes = 0;
541:   for (const entry of tree.entries) {
542:     if (
543:       entry.path.includes("\\") ||
544:       path.posix.isAbsolute(entry.path) ||
545:       entry.path
546:         .split("/")
547:         .some((segment) => segment === "" || segment === "..")
548:     ) {
549:       throw new Error(`Skill tree entry has unsafe path: ${entry.path}`);
550:     }
551:     if (paths.has(entry.path)) {
552:       throw new Error(`Skill tree contains duplicate path: ${entry.path}`);
553:     }
554:     paths.add(entry.path);
555:     const bytes = decodeTreeEntryContent(entry.contentBase64, entry.path);
556:     totalBytes += bytes.length;
557:     if (files.length + 1 > MAX_STAGED_SKILL_FILES) {
558:       throw new Error(
559:         `Skill tree exceeds max file count ${MAX_STAGED_SKILL_FILES}`,
560:       );
561:     }
562:     if (totalBytes > MAX_STAGED_SKILL_BYTES) {
563:       throw new Error(
564:         `Skill tree exceeds max byte count ${MAX_STAGED_SKILL_BYTES}`,
565:       );
566:     }
567:     files.push({ bytes, mode: entry.mode, relativePath: entry.path });
568:   }
569:   return files.sort((left, right) =>
570:     compareStringsByCodePoint(left.relativePath, right.relativePath),
571:   );
572: }
573: 
574: function hashStoredTreeFiles(files: readonly CollectedSkillFile[]): string {
575:   const hash = createHash("sha256");
576:   hash.update("bb-skill-tree-v1");
577:   for (const file of files) {
578:     hash.update("\0file\0");
579:     hash.update(file.relativePath);
580:     hash.update("\0");
581:     hash.update(file.mode.toString(8));
582:     hash.update("\0");
583:     hash.update(String(file.bytes.length));
584:     hash.update("\0");
585:     hash.update(file.bytes);
586:   }
587:   return hash.digest("hex");
588: }
```

## gui/apps/host-daemon/src/injected-skills.ts:668-808

SHA256 `3bdbf39d51b48e0f442bb7d1f9cb8ffa6557d640d55ac88ccadceac8d5e72076`

```text
668: async function writeFetchedTreeToStore(args: {
669:   dataDir: string;
670:   tree: HostDaemonSkillTree;
671:   treeHash: string;
672: }): Promise<string> {
673:   if (args.tree.treeHash !== args.treeHash) {
674:     throw new Error(
675:       `Fetched skill tree hash mismatch: expected ${args.treeHash}, received ${args.tree.treeHash}`,
676:     );
677:   }
678:   const files = validatedTreeEntries(args.tree);
679:   const actualHash = hashStoredTreeFiles(files);
680:   if (actualHash !== args.treeHash) {
681:     throw new Error(
682:       `Fetched skill tree content hash mismatch: expected ${args.treeHash}, computed ${actualHash}`,
683:     );
684:   }
685: 
686:   const storeRootPath = resolveSkillStoreRootPath(args.dataDir);
687:   const treeRootPath = resolveStoredTreeRootPath(args.dataDir, args.treeHash);
688:   await fs.mkdir(storeRootPath, { recursive: true });
689:   const tempRootPath = path.join(
690:     storeRootPath,
691:     `.tmp-${args.treeHash}-${process.pid}-${Date.now()}-${randomUUID()}`,
692:   );
693:   const contentRootPath = path.join(tempRootPath, STORE_CONTENT_DIR);
694:   await fs.mkdir(contentRootPath, { recursive: true });
695:   try {
696:     for (const file of files) {
697:       const destinationPath = path.join(contentRootPath, file.relativePath);
698:       await fs.mkdir(path.dirname(destinationPath), { recursive: true });
699:       await fs.writeFile(destinationPath, file.bytes, { mode: file.mode });
700:       await fs.chmod(destinationPath, file.mode);
701:     }
702:     await fs.writeFile(path.join(tempRootPath, STORE_LAST_USED_MARKER), "");
703:     await fs.writeFile(
704:       path.join(tempRootPath, STORE_COMPLETE_MARKER),
705:       "complete\n",
706:     );
707:     await fs.rename(tempRootPath, treeRootPath);
708:   } catch (error) {
709:     if (
710:       isFsErrorWithCode(error, "EEXIST") ||
711:       isFsErrorWithCode(error, "ENOTEMPTY")
712:     ) {
713:       await fs.rm(tempRootPath, { recursive: true, force: true });
714:     } else {
715:       await fs.rm(tempRootPath, { recursive: true, force: true });
716:       throw error;
717:     }
718:   }
719:   await touchStoredTree(treeRootPath);
720:   await gcSkillStore(args.dataDir);
721:   return path.join(treeRootPath, STORE_CONTENT_DIR);
722: }
723: 
724: export async function ensureStoredSkillTree(args: {
725:   dataDir: string;
726:   fetchSkillTree: FetchSkillTree;
727:   treeHash: string;
728: }): Promise<string> {
729:   const key = `${args.dataDir}\0${args.treeHash}`;
730:   const pending = pendingSkillTreePulls.get(key);
731:   if (pending) {
732:     return pending;
733:   }
734:   const pull = runInSerialLane(skillStoreQueues, args.dataDir, async () => {
735:     const treeRootPath = resolveStoredTreeRootPath(args.dataDir, args.treeHash);
736:     try {
737:       await fs.access(path.join(treeRootPath, STORE_COMPLETE_MARKER));
738:       await fs.access(path.join(treeRootPath, STORE_CONTENT_DIR));
739:       await touchStoredTree(treeRootPath);
740:       await gcSkillStore(args.dataDir);
741:       return path.join(treeRootPath, STORE_CONTENT_DIR);
742:     } catch (error) {
743:       if (!isFsErrorWithCode(error, "ENOENT")) {
744:         throw error;
745:       }
746:     }
747:     return writeFetchedTreeToStore({
748:       dataDir: args.dataDir,
749:       tree: await args.fetchSkillTree(args.treeHash),
750:       treeHash: args.treeHash,
751:     });
752:   }).finally(() => pendingSkillTreePulls.delete(key));
753:   pendingSkillTreePulls.set(key, pull);
754:   return pull;
755: }
756: 
757: export async function stageInjectedSkillSources(
758:   args: StageInjectedSkillSourcesArgs,
759: ): Promise<StagedInjectedSkills> {
760:   if (args.injectedSkillSources.length === 0) {
761:     return {
762:       catalogHash: EMPTY_SKILL_CATALOG_HASH,
763:       skillRoots: [],
764:     };
765:   }
766: 
767:   const logger = args.logger ?? createNoopLogger();
768:   const trees: CollectedSkillTree[] = [];
769:   const sortedSources = [...args.injectedSkillSources].sort((left, right) =>
770:     compareStringsByCodePoint(left.name, right.name),
771:   );
772:   const stagedTreeHashes = sortedSources.flatMap((source) =>
773:     source.kind === "tree" ? [source.treeHash] : [],
774:   );
775:   markActiveSkillTreeStages(args.dataDir, stagedTreeHashes);
776:   try {
777:     for (const source of sortedSources) {
778:       if (source.kind === "tree") {
779:         try {
780:           if (args.fetchSkillTree === undefined) {
781:             throw new Error("Skill tree fetch transport is unavailable");
782:           }
783:           const sourceRootPath = await ensureStoredSkillTree({
784:             dataDir: args.dataDir,
785:             fetchSkillTree: args.fetchSkillTree,
786:             treeHash: source.treeHash,
787:           });
788:           const skillFilePath = path.resolve(sourceRootPath, source.entryPath);
789:           if (!isPathWithinDirectory(sourceRootPath, skillFilePath)) {
790:             throw new Error(
791:               `Injected skill entry path escapes tree: ${source.entryPath}`,
792:             );
793:           }
794:           trees.push(
795:             await collectSkillTree({ source, sourceRootPath, skillFilePath }),
796:           );
797:         } catch (error) {
798:           logger.warn(
799:             {
800:               name: source.name,
801:               treeHash: source.treeHash,
802:               sourceType: source.sourceType,
803:               reason:
804:                 error instanceof Error && error.message.trim().length > 0
805:                   ? error.message
806:                   : "Unable to pull injected skill tree",
807:             },
808:             "Failed to pull required injected skill tree",
```

## gui/packages/host-daemon-contract/src/commands.ts:118-160

SHA256 `d2bbe6c7fccd926a19404727c123a738e6ef3f9d9e2fba863f89a04fbabde709`

```text
118: 
119: const hostDaemonInjectedSkillSourceBaseSchema = z
120:   .object({
121:     name: z.string().max(64).regex(INJECTED_SKILL_NAME_PATTERN),
122:     description: z.string().min(1).max(1024),
123:   })
124:   .strict();
125: 
126: export const hostDaemonInjectedSkillSourceSchema = z.discriminatedUnion(
127:   "kind",
128:   [
129:     hostDaemonInjectedSkillSourceBaseSchema
130:       .extend({
131:         kind: z.literal("tree"),
132:         treeHash: z.string().regex(/^[a-f0-9]{64}$/u),
133:         entryPath: z.string().min(1),
134:         sourceType: z.enum(["builtin", "data-dir"]),
135:       })
136:       .strict(),
137:     hostDaemonInjectedSkillSourceBaseSchema
138:       .extend({
139:         kind: z.literal("workspace-path"),
140:         sourceType: z.literal("project"),
141:         sourceRootPath: z.string().min(1),
142:         skillFilePath: z.string().min(1),
143:       })
144:       .strict(),
145:     hostDaemonInjectedSkillSourceBaseSchema
146:       .extend({
147:         kind: z.literal("host-path"),
148:         sourceType: z.enum(["shared-user", "shared-project"]),
149:         sourceRootPath: z.string().min(1),
150:         skillFilePath: z.string().min(1),
151:       })
152:       .strict(),
153:   ],
154: );
155: export type HostDaemonInjectedSkillSource = z.infer<
156:   typeof hostDaemonInjectedSkillSourceSchema
157: >;
158: 
159: const hostDaemonBridgeLaunchSchema = z
160:   .object({
```

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
