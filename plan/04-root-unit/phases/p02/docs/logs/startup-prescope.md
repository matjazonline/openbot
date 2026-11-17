# Plan04 p02 pre-scope; no p02 source edit before integration acceptance

Original Plan04 README/02 require Members|Public only, every channel.department_id
required/default root, no members_only, every desk materializes derived grants,
explicit configured grants on either mode; public anyone writes but only listed/root
members trusted; UI viewer still requires company membership and owner exemption.
Root member has grants on root desks, no automatic membership of all subdepartments.

IMPORTANT inherited user decision 01DEP-C002/r1 in Plan01 COORDINATION.md: ACTIVE,
user decision2026-11-10 (later than Plan04 original2026-11-09), targets p02/p03 and
plan04docs. One step BOTH WAYS: own members/all own role holders, parent Lead and
all direct child Leads, never further ancestors/descendants. Keep existing ELIGIBLE
in department_desks.rs155-184 and tests for child/grandchild/parent/grandparent. Do
not infer removal of child-lead access from Plan04's older one-step shorthand.

Current department_desks.rs skeleton: DeskOwnership32-49 optional department+flag;
stored_mode54-74; prelock114-144; ELIGIBLE155-184; eligible_on186-199;
lock_principals203-224; rederive_on230-314; sync_department319-336;
sync_desk339-345; set_ownership350-408. rederive_on ALREADY batches locks/writes,
but unconditionally deletes ALL derived grants and inserts full current set for
all touched desks (2capabilities), bumping each desk settings_revision once.
Public/nonmembers desks filtered out via members_only in rederive/sync. Required
root desks will materialize1000members×200desks×2caps=400kgrants; measure production
join/remove cost, not just fixture setup. Consider existing-layer differential
writes if full rewrite is slow; preserve locks, provenance, revision fencing and
notifications. Plan allows department-reference fallback if measured materialized
cost remains unacceptable, but avoid speculative schema/policy expansion.

ChannelAccessMode90-116 has Team/Allowlist/Public; participant_access140-159,
viewer_access179-197. Public opens mail, not unauthenticated UI read. Private
Members uses explicit/derived grants and owner; public trusts listed/current root
members. FromStr must reject removed spellings, no aliases.

Audit all data/string/SQL consumers as well as enum callers. Private old allowlist
fixtures under default root MUST move to a genuine restricted subdepartment;
changing allowlist->members while granting all root members would invalidate their
privacy assertions. Named addresses on root desks now add access rather than
restricting root members, as original plan intends. Root audience changes must
not affect old source membership-incarnation pins/direct-history privacy.

Nonnullable domain Channel.department_id and ChannelDb; ownership edit optional
new owner can mean unchanged, but clearing is gone. RawSQL NULL and foreign scoped
ids rejected. Company/direct-principal workflow associations may STILL have no
origin department; do not remove their escalation/role availability checks merely
because desks are now always owned. Map intentionally removed detach tests to
new required-owner/null-rejection tests and keep real runner inventory.

UI: required department selector/default root; no No department or members-only
flag/script; Open to anyone and Also allow these addresses. Reuse canonical @public
marker and existing normalization instead of adding a second persisted boolean.
Trace create/edit and type/schema/form save consumers, not only backend storage.

Architecture gate needed for cohesive changed required schema, policy and grant
transactions before source changes. Same author/root confirmed Sol6.1/xhigh;
retained independent B Astra/high. Phase docs own p02. Recheck coordinator and
ack04RU-C001/r1 plus inherited01DEP-C002/r1. Focus per batch policy/grants/private
interfaces/auth suites and1000×200 benchmark, full gate once after p03.
