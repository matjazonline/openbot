# AWC-P01-D1 r3 — implementation addendum A1

Root/A, 2026-11-22. Gate r3 stays the decision of record and is not edited. This addendum records
where batch AWC-P01-B1 placed three items differently from r3's text. Review accepted them on
the merits in note AWC-P01-B1-R1-N1 (finding F2 asked for this record). They change placement
and phase ownership only; they change no authority, level or contract decision.

1. **`MessageTriggerInput::schema()` declares no `fields`.** r3 D3, first bullet, said `schema()`
   declares it optional. That schema is the literal input schema of many responders, and as the
   exact `message.reply`/`wait.event` payload. Declaring `fields` there gave every such desk an
   unbounded intake: attempt 1 failed 192 tests. A responder declares its intake only in its own
   `input_schema.properties.fields`. A call with `fields` to a desk without one is
   `intake_not_declared`.
2. **The fields bound runs at binding configuration and activation, not at compile.** r3 D3,
   second bullet, said "a publication error". A version does not know it is message-triggered
   until a binding says so, and bounding every version's `fields` property would constrain
   unrelated workflows. `ConfiguredBinding::check_message_trigger`, which all four
   configure/activate paths call, refuses with `binding.intake_fields_invalid` or
   `binding.result_fields_invalid`, naming the full schema path.
3. **`channel.deny`, `channel.hand_off` and result-field validation move to p04.** r3's
   verification row put the `channel.deny` compile rules in p01. They are closure producers of
   the response pipeline, and a deny over mail is a final-response publication. Steps that
   finalization cannot handle yet would make mail runs that end in them fail. p04 owns:
   - both steps and their compile rules;
   - runtime validation of their `fields`;
   - `channel.hand_off` refusing a nested call that did not close `done`;
   - projecting the final output's `/fields` into the `done` and `handed_off` closures.

Also deferred, with owners:
- **p05 A:** the authoring catalogue entry and directory discovery (F9).
- **p02 / p03:** the DB-backed authority controls.
- **p03, replay:** `ChannelCallRequest::parse` checks the deadline against `now`, so a replayed
  call must resolve from its invocation record before re-parsing (F4).
- **p03, intake in `allOf` or `$ref`:** `declared_fields` reads only a direct
  `properties.fields`. An intake declared there counts as none, and calls with `fields` are
  refused, so this fails closed (F4).
