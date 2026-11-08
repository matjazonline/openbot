# Official API evidence checked 2026-11-08

https://developers.google.com/workspace/gmail/api/guides/configure-mcp-server
Observed: Developer Preview program prerequisite. Overview describes search/read/thread/label metadata and creating drafts/labels. No send promise used for workflow examples. A production Google MCP registration requires independently pinned endpoint/auth/scopes; preview docs alone do not authorize arbitrary Google bearer destinations.

https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.messages/send
Observed: metadata POST endpoint https://gmail.googleapis.com/gmail/v1/users/{userId}/messages/send; me denotes authenticated user. Request/response Message object. Examples use metadata URI with manually reviewed base64url MIME and explicit threadId, not upload URI or generated encoding.
