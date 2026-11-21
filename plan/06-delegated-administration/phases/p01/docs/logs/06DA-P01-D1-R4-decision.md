## 06DA-P01-D1/r4 — bounded desk-only fallback and competing grant proof

Recheck06DA-P01-D1-R3/N1 raised F2: add actual competing initial/current model
grant-revocation proof, and explicit direct-self/other-principal evidence. No r3
source edits have begun; input remainsba5e79b9…ddd48.

- Remove r3's proposed direct self-inbox fallback. Inspection of the actual existing
  principal_direct_can_view function (init schema16337) proves every current
  same-company person with canonical root membership already reads an agent direct
  target. A manager must have that same current person/root witness. Thus a valid
  managed agent target reaches the unchanged human-View success path; the added
  self exception would be redundant/unreachable. Person/foreign/missing targets do
  not become agent targets. The new fallback handles only a desk and requires both
  exact selected-agent desk View and Participate witnesses, never a principal inbox.
  Preserve existing valid agent-direct tests and other-principal/foreign refusals;
  no invented no-human-View direct-agent fixture or new direct-view permission.
- Strengthen the proof mapping: create a genuine owned automatic reply and obtain
  its claimed agent activation/frozen selection through the existing admission/
  claim/preparation fixtures. Exercise actual AgentExecutionPersistence::prepare_agent
  at the initial boundary and agent_preparation::current_on at the checkpoint boundary.
  For each boundary, hold a real grant-deletion transaction before authorization,
  observe the model caller waiting on that exact grant revoker's backend, commit
  revocation and assert refusal/no provider/output. In the opposite ordering, hold
  the actual successful current-authorization transaction open, start a competing
  direct grant DELETE, observe it waiting on the authorization owner, then commit;
  verify old-grant authorization followed by completed removal and refusal on the
  next check. Actor management and agent definition/revision remain otherwise valid.
  Run View/Participate cases so either lost witness is decisive. Use disjoint owned
  databases and real row-lock wait sampling, never timing-only sleeps or sequential
  booleans. A full native Deployment positive remains independent behavioral proof.
- Add manual/observer/schedule no-owned-proof negative coverage using actual run
  provenance: actor View revoked while management and agent grants remain valid
  must still refuse at initial/current model boundaries. Actual provider call count
  and publication counts remain0. Existing privacy/history readers unchanged.

This supersedes only r3's redundant direct-self branch and incomplete verification
mapping; the desk policy and all r2/r3 fences remain. F2 needs B's explicit r4
recheck; initial/current grant fencing is not accepted on inspection alone.
