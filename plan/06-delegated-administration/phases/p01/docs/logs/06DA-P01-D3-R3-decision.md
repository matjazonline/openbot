# 06DA-P01-D3/r3 — correct the agent preflight port assumption

Reopens ONLY D3/r2 item5's named application preflight dependency. Original
README/01B.4–5 and the accepted r2 ownership/schema/construction/proof decisions
remain. B2is accepted atf90321fd…972d0c. No Plan05source imported; user stop-at03
is unchanged. B3source is intermediate/unaccepted, not an implementation review.

Actual AgentUseCases fields (agent.rs:251–265 before this batch) have company,
agent and draft persistence handles; there is NO participant persistence handle.
R2's sentence naming participant AdministrationPersistence cannot be implemented
without an extra constructor/container dependency. Traced CompanyPersistence
callers before selecting the existing responsible owner instead. Root typed
CompanyPersistence already owns organization access and is passed to this use case.

Decision: extend existing CompanyPersistence with the existing required
AdministrationPersistence supertrait. Agent management preflights use that same
company persistence handle, with actual Agent/Unit/Root/AnyUnit targets. No new
constructor parameter, service, container wiring, optional fallback, JSON field,
role or SQL authority implementation. PostgresPersistence already implements the
common authority; only test implementations gain an explicit authority behavior.
Current literal implementation inventory is production company.rs and shared
company_test_support::MockCompanyPersistence; all-target compile must enumerate
any nonliteral implementations. Existing AGENTS historical count is not proof.

MockCompanyPersistence's explicit authority method derives Owner/Admin from its
actual company_access/membership fixture, propagating its configured outages;
it models root managers only and does not prove unit traversal or foreign target
ownership. Actual Postgres/route/service regressions remain required for subtree,
placement, foreign/agent, no-View, revocation and direct persistence proof. No
silently-successful default or blanket production implementation is introduced.

Do not replace every CompanyPersistence consumer's authorization policy. In this
batch only AgentUseCases management preflights consume this newly available
capability. Owner-only company creation/deletion/billing/memory tenant and root
configuration paths retain their actual current policy; read/classification helpers
and UI consumer migration still belong to their original phase/batch. Existing
managed_company behavior is semantically root owner/admin; final removal of the
old membership predicate still requires its complete consumer disposition, not a
sweep here. No permissive default is added to preserve old test setup.

Additional typed realization under unchanged r2 decision: distinguish definition
creation from editing. Internal DefinitionCreation/CreateDefinition and CopyTemplate
resolve the actual creating department; ManageDefinition(workflow) requires an
existing target. Thus an unauthorized/missing EDIT cannot use a missing-row creation
fallback to expose revision conflicts. Public directory captures can prepare a new
draft, selecting creation scope only for an actually missing company-qualified row,
and existing stored/bound authority otherwise. All cases remain under the common
person/company lock and predicate; no new persistent/public permission vocabulary.
This is the same r2 creation/existing ownership decision, with the distinction
made explicit at the type boundary. Add enduring missing/foreign/stale edit tests.

Required verification: original D3/r2 real matrix plus affected owner/admin/member
application and company fixture tests; compile every CompanyPersistence consumer;
explicit database and JSON authority proof rather than treating the root-only
shared fake as proof of hierarchical authorization. Same whole-B3review/final
checkpoint requirements apply. No B3implementation acceptance or full-plan green
is sought at this gate.
