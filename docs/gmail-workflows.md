# Gmail HTTP workflow examples

For setup, account recovery, data handling and key custody, see
[private integration accounts and operations](vault-integrations.md).

The runnable source and inert sample inputs are in
[read and prepare](../samples/gmail-read-prepare.json) and
[reviewed send](../samples/gmail-reviewed-send.json). Copy the `source` object into
the workflow editor, assigning a new workflow UUID for each definition. Use the
`sample` object in the sealed sample runner. Fixture addresses, IDs and message
content are illustrative; samples never contact Google or resolve a vault entry.

## Connect and authorize a connection

Sign in to Openbot, open `/ui/profile/vault`, and connect the Gmail app. The
mailbox may differ from your Openbot login. Connecting stores your private account;
it does not grant a company permission to use it.

A company administrator can create an HTTP connection with an authenticated,
same-origin JSON `POST /api/companies/{company}/http-connections`:

```json
{"origin":"https://gmail.googleapis.com"}
```

The response includes its `id` and `revision`. Register a deterministic operation
with `POST /api/companies/{company}/http-connections/{id}/operations`. For sending,
use the current revision and keep native action approval enabled:

```json
{
  "expected_revision": 1,
  "name": "http.request",
  "input_schema": {
    "type":"object",
    "properties": {
      "method":{"const":"POST"},
      "path":{"const":"/gmail/v1/users/me/messages/send"},
      "headers":{"type":"object"},
      "body":{"type":"object","properties":{
        "raw":{"type":"string","minLength":1,"maxLength":8192,"pattern":"^[A-Za-z0-9_-]+$"},
        "threadId":{"type":"string","minLength":1,"maxLength":128}
      },"required":["raw","threadId"],"additionalProperties":false}
    },
    "required":["method","path","body"],
    "additionalProperties":false
  },
  "output_schema": true,
  "effect": "write",
  "approval_required": true
}
```

Create a separate read connection/operation, with `effect: "read"`, `method`
constrained to `GET`, and `path` constrained to the thread endpoint used by the
read example. This keeps the registered read contract from permitting sends.
Registration changes the connection revision; stale writes fail and must be
reviewed against the current connection. The endpoints accept no credentials.

Open `/ui/companies/{company}/http-connections/{id}`, choose its credential source,
select Gmail, and explicitly confirm the exact destination and company using your
own connected account. Other administrators can inspect the approved binding's
label/status, but cannot browse your vault or read a credential. Register operations
before consent; a changed connection requires its current revision.

In workflow publication/binding, select that approved connection for the `service`
HTTP resource and the registered `http.request` operation. The read example also
requires a published agent named `assistant`; configure it with no business tools,
appropriate model instructions and permission to process the selected mailbox text.
The protected send example already includes `with.approval`, using the binding's
`params.reviewers` and the run deadline. Replace the sample reviewer UUID with real
authorized company person principals in the saved binding parameters:

```json
{"reviewers":[{"kind":"principal","id":"REVIEWER_PRINCIPAL_UUID"}]}
```

Its sample response declares `approval: true` to simulate review, never to approve
a production run. Approval covers the frozen business operation, account and arguments. Token refresh
occurs at execution after review; it does not approve changed inputs or consent.
Publication, direct agent tools, and samples cannot refresh or borrow the account.

## Read and prepare

Supply a safe `thread_id` (letters, digits, `_`, `-`; at most 128 characters). The
workflow uses the supported `concat` binding to construct a relative thread path
under the fixed Gmail origin. It requests `format=full` with a field mask selecting
thread ID, message IDs and snippets. It passes only the first message's selected
snippet and IDs to the model, then returns suggested text for human review. Missing selected fields return an explicit unavailable result without a model
call; a malformed or oversized response fails. The native HTTP response bound is 32 KiB. This minimal example does
not page through a mailbox or download attachments.

The API path and `me` user alias follow Google's
[thread get reference](https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.threads/get).

## Send a reviewed message

Prepare and inspect the RFC 5322 MIME message outside the workflow. Supply its
unpadded base64url encoding as `raw`, plus the existing Gmail `thread_id`. The
example posts `{"raw":...,"threadId":...}` to the fixed
`/gmail/v1/users/me/messages/send` path and returns Google's bounded message
identity. The fixture demonstrates CRLF headers, Subject, Message-ID,
In-Reply-To and References. Replace its sender, recipients, headers and body with
reviewed values; do not send the fixture. The workflow validates encoding shape
and size, not the semantic correctness of arbitrary supplied MIME. No workflow
MIME/base64 transform exists, and model text is not trusted encoding.

Google requires the thread ID, matching subject and appropriate reply headers for
thread association. See the [Message resource](https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.messages)
and [send method](https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.messages/send).
A stable Message-ID is correlation only. A timeout after dispatch may have sent the
message: inspect/reconcile the native action receipt instead of sending it again.
Refreshing a token never makes an uncertain send safe to replay.

## MCP is a separate integration

The production Gmail app supports the HTTP API only. Its token cannot be attached
to a user-selected MCP server. Static bearer accounts can use explicitly
owner-approved MCP endpoints through the same private gateway. Owner-authorized
catalog refresh returns bounded tool metadata, preserves reviewed grants, and
cannot make business calls or reveal tokens.

Google's [Gmail MCP guide](https://developers.google.com/workspace/gmail/api/guides/configure-mcp-server)
currently describes Developer Preview enrollment, read/search/thread/label and
draft/label operations. It does not establish an available send tool. This release
does not register that server or claim its live catalog/authentication is verified.
An equivalent MCP example requires a separately registered, pinned app policy and
a reviewed actual `tools/list` catalog with matching scopes/schema. Use `mcp.call`
with the discovered exact tool name and arguments; do not invent a Gmail tool name.
The HTTP examples require no MCP preview enrollment.
