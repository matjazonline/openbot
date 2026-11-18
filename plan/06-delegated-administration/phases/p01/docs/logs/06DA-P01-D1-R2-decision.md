## 06DA-P01-D1/r2 — downstream department effects (supersedes r1 scope)

Independent 06DA-P01-D1-R1/N1 raised high F1: root/read guards in department
provisioning and lead-follow side effects invalidate the complete-owner boundary.
This revision retains decisions1–6 but expands B1's implementation and proof scope
instead of bypassing the nested boundaries. F1 remains open until B rechecks r2.

1. Extend the same required administration lookup port with a typed target:
   Root, Unit(DepartmentId), Agent(AgentId). Root/unit use the one SQL predicate.
   Agent resolution proves a current same-company agent principal, unions its real
   department memberships and roles, and applies that predicate to those units;
   only an agent in no department falls back to root. Unknown/foreign agents never
   get the fallback. This is original README decision2, required here by automatic
   initial publication and follow-lead re-publication. Preserve the publication
   account→company→agent locking and revision/skill/tool/receipt fences.
2. Central persistence authority distinguishes locking current person facts from
   requiring root management. Add explicit scoped unit/agent/association authority
   paths over the same predicate; do not make authorize_company mean "any lead".
   Existing root-owned resource/configuration callers keep root permission. Every
   migrated caller supplies its actual department/agent or immutable run association.
   A run-scoped management guard reads its association under the company lock and
   preserves all live actor, run, lease, source and credential checks. NOWAIT callers
   retain their mode and sorted account prefix (l-070f6689); no late blocking helper.
3. Department-owned desk creation first creates the plain desk directly in its
   owning department, then configures its responder through the existing follow-lead
   flow. Set the initial lead role before responder publication so the selected
   agent is already placed in the new unit. This avoids a temporary root-owned desk
   or implicit root permission and consolidates the plain/agent creation paths.
   Existing callers retain unique command keys, exact provisioning receipts,
   grant synchronization, native source compilation, publication and binding state
   validation. Lead replacement writes placement before follow-lead as today.
4. Department follow-lead, binding save/check_run_as, reply authorization/selection,
   native template captures and agent publication must use the owning target's
   management guard. Configuration-only operations prove association existence,
   tenant and exact thread/inbox relationship without requiring or granting a
   conversation View grant. Inspect/manual message/history and actual read routes
   retain their existing grant checks. Do not change shared association visibility
   helpers to silently allow managers. If an existing combined helper serves both
   configuration and reading, split the responsibilities at the owning layer.
5. Native automated reply admission/preparation/continuation consumers of the newly
   valid lead run-as must ask scoped authority for the captured run association,
   rather than root authority. Scope the changed callers through actual producer
   and run/fence traces. Retain actual agent receiving/participation grants,
   classification and consumption checks, current principal/member incarnation,
   frozen publication, source-proof, resource credential and ownership fences.
   A configuration manager gains no human history/desk-read API entitlement.
6. Added proof: real delegated create with no/person/agent lead; transition
   person→agent, agent→agent, agent→person, agent→none; initial and already-published
   selected agent variants; lead run-as retained in exact provisioning/binding
   revisions; department manager lacking desk View can change configuration while
   thread/history/desk reads stay refused; one real automated message admission and
   execution through the delegated department desk, proving the new run-as survives
   runtime guards. Direct binding/reply/publication sibling and foreign target calls
   refuse. Preserve root-only resource checks with a negative lead case. The batch
   is not accepted if any required downstream case is left for a future batch.

No Plan05 scope data is involved. Knowledge stays explicitly untouched. Workflow
fully general definition creation/active-binding ownership and public route/UI
wiring remain later independent batches; user chose active-bound-unit authority.
If actual native template construction needs that definition contract, include its
owning schema/decision in a reopened gate before dependent edits, never substitute
root identity or skip the authorizer. Full final gate remains at the complete-plan
boundary; checkpoint gates and affected interfaces must pass before dependency stop.
