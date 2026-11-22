---
title: "Archiving a unit lets its parent's manager cancel the subtree's open runs, and its desks can still be deleted or moved out"
author: Mac02
date: 2026-11-21
branch: plan08_ph1
cost: { minutes: 21, tokens: 55273 }
id: l-da2fee04
scope: repo
status: active
src: agent
---

## Decided
Archiving a unit refuses while any run on its subtree's desks is unfinished, unless the archiver chooses "cancel open runs and archive". The user decided that the parent unit's manager may cancel those runs even on desks they cannot read: archiving authority covers ending the work, every cancelled root run records a `control_cancel` event naming the archiver, and reading the runs still needs desk access.
An archived unit's desks may still be deleted, or moved out to an active unit by someone who manages both units, because deleting a unit requires it to own no desks. Moving a desk in, and every other desk configuration, is refused with `unit.archived`.

## Tried and ruled out
Limiting cancel-and-archive to runs the archiver can access: rejected by the user, since a manager above a members-only desk could then never archive without asking the desk's people first.
Freezing an archived unit's desks entirely, or allowing delete but not move-out: rejected, because an archived unit with desks could then only be deleted after restoring it, and resources already may move out (02C.2).

## Watch out
Cancel-and-archive refuses (`unit.archive_open_work`) while a native MCP broker run is open (its cancel belongs to the local agent) or when more than 64 root runs would need cancelling.

In the code: plan/08-goals/phases/p02/docs/TASK.md decisions U3/U4 and D1.3/D1.8 (gate D1-r3, review note 08G-P02-D1-R3-N1).
