---
title: "Check an invitation is still pending before rechecking the inviter's role"
author: Mac02
date: 2026-11-18
branch: plan06
cost: { minutes: 4, tokens: 16827 }
id: l-6c1fb59a
scope: repo
status: active
replaces: l-1a464501
src: agent
touches:
  - src/adapters/persistence/company_invite.rs
  - src/adapters/persistence/company_level_authority.rs
---

## Decided
Invitation writes that edit an existing invite (full edit and level-only edit) return "no change" for a closed invitation first. Only then do they recheck whether the actor may write that invitation's role. Root admins may write member invitations only; any invitation carrying the admin role stays the owner's.

## Tried and ruled out
Rechecking the inviter's role right after the company lock, before the status check. The existing acceptance-versus-level-edit race test failed with Forbidden. Accepting a member invite addressed to the acting admin demotes that admin to member before the edit takes the company lock, so the edit then saw a plain member actor. The protocol's contract is that a closed invitation is never rewritten and reports "no change", not a refusal.

## Watch out
Creation upserts by address. An invitation created by another writer while this one waited for the company lock is invisible to the earlier row lock. Authorize creation against a plain read taken after the company lock; every role and level writer holds that lock. Do not re-lock the invite row after the company lock: edit, level edit and delete lock the invite row before the company, so a row lock taken after the company can deadlock with them. Acceptance takes the company first and does not clash.

In the code:
- src/adapters/persistence/company_invite.rs: create_invite, update_invite, set_invite_access_level, delete_invite
- src/adapters/persistence/company_level_authority.rs: require_inviter, read_invite_by_email, refuse_admin_address
- Exposing test: access_level_acceptance_and_own_invite_assignment_do_not_deadlock_or_restore_levels
- Competing tests: delegated_admin_competing_owner_admin_invite_precedes_admin_member_upsert, its reverse, and delegated_admin_competing_owner_promotion_precedes_admin_invite_delete
