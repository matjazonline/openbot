# Bound HydraDB Requests and Responses

## Goal

Put explicit byte/character/token and time bounds around every HydraDB request and response so one
message, provider response, or configuration value cannot cause excessive allocation, cost, or
worker starvation.

## Current Risk

Recall sends the complete latest prompt, persistence copies full user/upstream context and final
answer into multipart bodies, and successful responses are deserialized without a body-size cap.
The 16,000-character memory-context cap is applied only after the entire response has been read and
parsed.

## Boundary Policy

Define and document separate limits for:

- recall query characters or prompt tokens;
- persistence user-context characters;
- persistence assistant-answer characters;
- additional context characters;
- maximum target collections per operation;
- maximum HTTP request bytes;
- maximum HTTP response bytes;
- maximum returned rows and per-chunk characters;
- connect, request, and total worker-operation timeouts.

Use UTF-8-safe truncation with an explicit marker when truncation is acceptable. Reject instead of
truncate where removing data would make the operation semantically misleading. Limits should be
provider-neutral application policy with adapter-level byte enforcement as defense in depth.

## Implementation Steps

1. Introduce named limit types/constants in the application/domain boundary rather than scattered
   numeric literals.
2. Build bounded `MemoryRecallQuery` and `MemoryConversation` values before entering the provider
   port. Make oversized construction explicit and testable.
3. Cap upstream pipeline context independently so multi-channel runs cannot grow memory input
   quadratically.
4. In the HydraDB adapter, check `Content-Length` when present and stream response bytes with a hard
   cap before JSON deserialization. Reject over-limit bodies as a safe typed provider error.
5. Validate result count against the requested maximum and cap each chunk before allocating the
   final formatted context.
6. Enforce a maximum multipart body size before sending; do not rely only on HydraDB rejecting it.
7. Add safe metrics for truncation/rejection and response-size failures without logging content.
8. Add upper timeout bounds coordinated with the lease supervisor in
   `05-supervise-memory-job-leases.md`.
9. Document limits in `.env.example` and deployment documentation if they are configurable.

## Tests

- Boundary tests for exactly-at-limit and one-over-limit query, user context, answer, and additional
  context, including multi-byte Unicode.
- Mock-server responses with absent, valid, deceptive, and oversized `Content-Length` values.
- Chunked responses that cross the byte cap must stop reading and return a typed bounded error.
- A provider returning more rows than requested or one huge chunk cannot exceed the final context
  budget.
- Multipart request construction cannot exceed its configured cap.
- Multi-collection persistence remains within a known aggregate concurrency and byte budget.
- Timeout upper/lower validation and a never-ending response body do not hold a worker forever.

## Acceptance Criteria

- Every HydraDB request and response has an enforced size and time bound.
- Limits are applied before large cloning, serialization, or JSON allocation where practical.
- Unicode truncation is safe and observable.
- Oversized provider responses cannot grow memory beyond the documented cap.
- Tests cover streaming responses, misleading headers, and aggregate multi-collection work.

