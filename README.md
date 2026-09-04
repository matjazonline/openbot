# Mail Agents Server

A transport-neutral agent automation platform built in Rust. Email is the initial/default channel
binding; canonical channels, actors, messages, threads, and deliveries are not email-shaped.

## Architecture Glossary

- **Channel:** The company-scoped business object that owns agents, policy, memory, threads, and
  access rules. It is not an email mailbox or chat conversation.
- **Transport kind:** An interface family such as email or Slack.
- **Channel binding:** One independently enabled, addressable transport interface on a channel.
- **Principal:** A stable company-scoped actor: person, agent, external person, or system actor.
- **Qualified identity:** The way one transport names a principal, scoped by transport, namespace,
  and subject.
- **Channel selector:** Transport-neutral intent to address a business channel. An email address is
  one adapter syntax that can resolve to a selector, not the identity of the channel.

The normative identity, authorization, threading, ingress, and delivery decisions are recorded in
[Transport Architecture Contract](docs/transport_architecture.md).

## 1. System Architecture Overview

The system operates as an asynchronous, event-driven pipeline that maps authenticated transport
events into canonical messages and a stateful LLM context, then exposes results through durable
transport deliveries. The first complete adapter supports SMTP and SendGrid email.

### 1.1 Core Subsystems

- **Inbound Mail Transfer Agent (MTA) / Webhook Gateway:** Listens for incoming traffic, terminating the SMTP connection and passing raw MIME payloads.
- **Message Parser & Normalizer:** Dissects MIME parts, sanitizes HTML to markdown/text, strips historical email quotes, and maps RFC headers.
- **State & Thread Manager:** Reconstructs canonical conversations from binding-qualified provider keys and manages principal access control lists per thread.
- **Agent Orchestrator:** The intelligent routing layer (detailed in `AGENTS.md`).
- **Delivery Worker:** Claims generic deliveries; the email adapter renders MIME, applies spoof-safe "From" formatting, manages thread headers, and dispatches through SMTP.

## 2. Email Standards & Protocol Handling

To ensure native compatibility with clients like Gmail, Outlook, and Apple Mail, the system must strictly adhere to Internet Message Formats.

### 2.1 RFC 5322 (Header Management)

To maintain the illusion of a standard group chat, the outbound dispatcher must perfectly mimic human reply headers.

- **Message-ID:** Every email part has a stable RFC Message-ID within its binding. Canonical messages use UUIDs and do not require an RFC Message-ID.
- **In-Reply-To:** Outbound agent messages MUST set this header to the `Message-ID` of the human email that triggered the execution.
- **References:** Outbound messages MUST append the triggering human's `Message-ID` to the existing chain of references. This builds the hierarchical thread tree in the client.

### 2.2 RFC 2045 (MIME Multipart Parsing)

Emails are rarely flat text. The Inbound Parser must handle `multipart/mixed` and `multipart/alternative`.

- **Text Extraction:** Prefer `text/plain` if present. If only `text/html` exists, pass through a DOM-to-Markdown converter to optimize token usage.
- **Attachment Handling:** Extract `Content-Disposition: attachment`. Hash the binary, store in an object store, and append an abstraction (e.g., `[Attachment: filename.pdf, URL: ...]`) to the agent's context window.

### 2.3 Quote Stripping (The "Reply-Chain" Problem)

Email clients append the entire historical thread below newly typed text. Feeding this into an LLM causes exponential context duplication.

- **Logic:** Implement heuristics to truncate the message at common splitters (e.g., `On <date>, <user> wrote:`, `-----Original Message-----`, or `>` blockquotes).
- **Fallback:** Compare the raw text against the stored DB history for that thread and perform a diff subtraction to isolate the net-new tokens.

## 3. Implemented Subsystems & Features

### 3.1 Inbound Webhook & Message Parser
- **MIME Dissection & Normalizer (`EmailParser`):** Extracts RFC 5322 headers (`Message-ID`, `In-Reply-To`, `References`, `Cc`, `Thread-Index`). Automatically converts HTML bodies to Markdown via `htmd` to optimize LLM tokens.
- **Quote Stripper:** Combines heuristic markers (`On ... wrote:`, `> blockquotes`, `-----Original Message-----`) with thread history line subtraction. Automatically bypasses quote stripping for forwarded emails (`Fwd:`) and for the initial email in a new thread.
- **Attachment Filter:** Differentiates document attachments from inline signature icons (ignores images < 10KB or inline CIDs) and formats prompt descriptors (`[Attachment: <name>, SHA256: <hash>]`).
- **SendGrid Webhook Idempotency:** Detects duplicate SendGrid webhook redeliveries (`Message-ID`) and rejects duplicates without re-processing.

### 3.2 State & Thread Manager
- **Thread Resolution & ACLs:** Reconstructs conversational graphs by matching `In-Reply-To`, `References`, or Outlook's base64 `Thread-Index` header. Enforces company & workflow participant email ACLs.
- **Role Assignment:** Distinguishes human users (`MessageRole::Human`) from automated agents (`MessageRole::Agent`) and system notifications (`MessageRole::System`).

### 3.3 Loop Guard Engine & Inter-Workflow Communication
- **RFC 3834 & Exchange Headers:** Outbound emails include `Auto-Submitted: auto-replied` and `X-Auto-Response-Suppress: All`.
- **Inter-Workflow Routing:** Attaches `X-MailAgents-Workflow-ID`, `X-MailAgents-Hop-Count`, and `X-MailAgents-Trace`. Supports collaborative communication between workflows while enforcing a **5-hop limit** (`MAX_WORKFLOW_HOPS`) and **cycle detection**.
- **Thread Turn Limit:** Enforces a maximum of 20 messages per thread within 1 hour (`MAX_THREAD_MESSAGES_PER_HOUR`) to halt ping-pong loops between automated systems.

### 3.4 Background Task Queue & Worker
- **Durable Task Store (`background_tasks`):** Ingests inbound emails synchronously and enqueues background processing tasks, allowing the webhook to return `HTTP 200 OK` in < 100ms.
- **Task Worker Poller (`TaskWorker`):** Independent loops keep long agent runs from holding up anything else. The task loop continuously fills up to `TASK_WORKER_CONCURRENCY` execution slots (default 4), polling an empty queue every 500ms and refilling a slot immediately when a task finishes. A schedule loop fires due recurring runs every 2 seconds, and a maintenance loop reaps expired task leases and checks quorum timeouts every 30 seconds. A failed poll backs off for 5 seconds. A failed task is retried after 60 seconds and then 120 seconds; the third failed attempt transitions it to `dead_letter`.
- **Delivery Worker (`DeliveryWorker`):** A queue of its own, because a delivery outlives the task that produced it and plenty of deliveries have no task behind them. It claims up to 20 deliveries every 500ms, orders the batch round-robin across companies so one tenant's burst cannot fill it, and for each delivery walks its frozen parts through the registered transport. Every write is fenced on the execution the claim minted. A sweep every 30 seconds charges an attempt to leases that expired and dead-letters deliveries whose predecessor can never land.
- **Leased Execution:** Claims use `FOR UPDATE SKIP LOCKED` and a 15-minute lease. Background executions renew the lease every 5 minutes, and an expired lease can be reclaimed by another worker.
- **Shutdown:** `Ctrl+C` or SIGTERM stops admission, cancels in-flight provider work, records leased durable tasks as retryable, and joins the task worker, SMTP listener, mailbox listener, memory worker, and runtime sampler within the bounded drain window.

#### Task Execution Flow

```mermaid
flowchart TD
    subgraph Ingress[Inbound ingestion]
        SMTP[SMTP listener<br/>envelope and DMARC verdicts]
        SG[SendGrid webhook<br/>raw MIME reverified here]
        APP[Mailbox or simulation compose<br/>authenticated by the signed-in principal]
        RELAY[Internal channel relay<br/>in-process, never leaves the building]
        PROVIDER[Signed provider webhook]
        INBOX[(inbound_events<br/>durable inbox, fast acknowledgement)]
        IEW[Inbound event worker<br/>claims 8, 120s lease, 90s deadline<br/>4 at once, 2 per company, 1 per installation]
        PRE[Preflight<br/>ingress guard, address resolution, principal ACLs,<br/>thread and turn policy — nothing written]
        VERDICT{Preflight verdict}
        REJECT[Rejected<br/>refused in-session or bounced<br/>no message, no thread, no task]
        UPLOAD[Store attachments<br/>content addressed, before any row exists]
        COMMIT[One transaction<br/>threads, canonical message, associations,<br/>outreach transitions, task, frozen deliveries]
        OUTCOME{Commit outcome}
        DUPLICATE[Redelivery<br/>returns the first ids and enqueues nothing]
        FILED[Filed only<br/>context-only message or outreach reply<br/>no task]
        QUEUED[Task enqueued<br/>email_agent_dispatch]

        SMTP --> PRE
        SG --> PRE
        APP --> PRE
        RELAY --> PRE
        PROVIDER --> INBOX --> IEW --> PRE
        PRE --> VERDICT
        VERDICT -->|rejected| REJECT
        VERDICT -->|accepted| UPLOAD --> COMMIT --> OUTCOME
        OUTCOME -->|duplicate| DUPLICATE
        OUTCOME -->|filed| FILED
        OUTCOME -->|answered| QUEUED
    end

    subgraph TaskQueue[Durable task queue]
        PENDING[(pending<br/>run_at gates availability)]
        PROCESSING[(processing<br/>worker id and 15-minute lease)]
        COMPLETED[(completed)]
        DEAD[(dead_letter)]
        APPROVAL[(pending_approval)]
        WAITING[(waiting_for_third_party_reply)]
        STOPPED[(stopped)]
    end

    QUEUED --> PENDING

    subgraph Worker[TaskWorker — three independent loops]
        TASK_TICK[Task loop every 500ms<br/>refills free execution slots, 4 by default]
        CLAIM[Claim pending rows only<br/>round-robin across companies<br/>FOR UPDATE SKIP LOCKED]
        SCHED_TICK[Schedule loop every 2s]
        SCHED[Claim and advance due schedules<br/>60s materialization lease]
        MAINT_TICK[Maintenance loop every 30s]
        REAP[Reap expired task leases<br/>charges an attempt and backs off]
        QUORUM[Decide due quorum waits]
        CENSUS[Publish the stuck-work census]

        TASK_TICK --> CLAIM
        SCHED_TICK --> SCHED
        MAINT_TICK --> REAP --> QUORUM --> CENSUS
    end

    PENDING -->|due| CLAIM
    CLAIM --> PROCESSING
    SCHED -->|scheduled_agent_run| PENDING
    PROCESSING -->|lease expired| REAP
    REAP -->|attempts left| PENDING
    REAP -->|attempts exhausted| DEAD

    subgraph Execute[Claimed task execution]
        LOAD[Decode the payload and reload<br/>the channel's current configuration]
        IDEMPOTENT{Outbound reply<br/>already saved?}
        REUSE[Associate the saved reply<br/>with any thread still missing it]
        AGENTS[Run each matched channel agent<br/>in pipeline order, memory recalled per turn]
        HITL{Approval or outreach<br/>requested?}
        PLAN[Freeze the reply and its delivery parts<br/>idempotency key derived from the task]
        COMMIT_RUN[Fenced transaction<br/>reply message, thread associations,<br/>deliveries and audit payload]
        MEM[Persist memories after the commit]
        RESULT{Run outcome}

        LOAD --> IDEMPOTENT
        IDEMPOTENT -->|yes| REUSE
        IDEMPOTENT -->|no| AGENTS --> HITL
        HITL -->|no| PLAN --> COMMIT_RUN --> MEM --> RESULT
        REUSE --> RESULT
    end

    PROCESSING --> LOAD
    PROCESSING -. lease renewed every 5 minutes .-> PROCESSING
    RESULT -->|replied, or nothing to answer| COMPLETED
    RESULT -->|failed, timed out, interrupted, lease lost| RETRY{Attempts exhausted?}
    RETRY -->|no — 30s doubled per attempt, so 60s then 120s| PENDING
    RETRY -->|yes — third attempt, or a terminal failure| DEAD

    subgraph Pauses[Approval, waiting and manual control]
        APPROVAL_ACTION{Approval link action}
        REPLY[Authorized third-party reply recorded]
        TIMEOUT[Quorum expired below its threshold]
        STOP[Manual stop]
        RESUME[Manual resume]

        APPROVAL --> APPROVAL_ACTION
        APPROVAL_ACTION -->|approve, default, or proceed partial| PENDING
        APPROVAL_ACTION -->|extend| WAITING
        APPROVAL_ACTION -->|reject| STOPPED
        WAITING --> REPLY --> PENDING
        WAITING --> TIMEOUT --> APPROVAL
        STOP --> STOPPED
        STOPPED --> RESUME --> PENDING
    end

    HITL -->|approval required| APPROVAL
    HITL -->|awaiting replies| WAITING
    QUORUM -. finds due waits .-> TIMEOUT
    PENDING -.-> STOP
    PROCESSING -. a stop clears the lease and fences the in-flight run out .-> STOP
    APPROVAL -.-> STOP
    WAITING -.-> STOP

    subgraph Delivery[Generic delivery queue]
        OUT_PENDING[(pending<br/>held until its dependency lands)]
        OUT_RETRYABLE[(retryable<br/>available_at backoff)]
        OUT_SENDING[(sending<br/>2-minute lease)]
        OUT_DELIVERED[(delivered)]
        OUT_UNKNOWN[(outcome_unknown<br/>never auto-retried)]
        OUT_DEAD[(dead_letter)]
        DEL_TICK[Delivery loop every 500ms]
        DEL_CLAIM[Claim up to 20<br/>at most 4 per company]
        PART[Per frozen part: fence the start,<br/>call the provider, fence the result]
        SENT{Provider outcome}
        DEL_SWEEP[Sweep every 30s<br/>expired leases and orphaned dependencies]

        DEL_TICK --> DEL_CLAIM
        OUT_PENDING -->|dependency delivered| DEL_CLAIM
        OUT_RETRYABLE -->|available_at reached| DEL_CLAIM
        DEL_CLAIM --> OUT_SENDING --> PART --> SENT
        SENT -->|every part delivered| OUT_DELIVERED
        SENT -->|definitely refused| OUT_RETRYABLE
        SENT -->|acceptance ambiguous| OUT_UNKNOWN
        SENT -->|terminal, or the fifth attempt| OUT_DEAD
        DEL_SWEEP -. expired before the request started .-> OUT_RETRYABLE
        DEL_SWEEP -. expired after it started .-> OUT_UNKNOWN
        DEL_SWEEP -. dependency gone terminal .-> OUT_DEAD
    end

    PRE -. system address answered .-> OUT_PENDING
    REJECT -->|bounce| OUT_PENDING
    COMMIT_RUN -->|the agent reply| OUT_PENDING
    AGENTS -. outreach question .-> OUT_PENDING
    APPROVAL -. approval request mail .-> OUT_PENDING
    SCHED -. scheduled run mail .-> OUT_PENDING
    PART -. addressed to another channel of this company .-> RELAY

    subgraph Shutdown[Shutdown]
        CTRL[Ctrl+C or SIGTERM]
        BROADCAST[Broadcast stop<br/>20s drain grace, then abort]
        DRAIN[Task, delivery, inbound-event, memory,<br/>SMTP, mailbox and sampler loops stop claiming]
        GIVEBACK[In-flight tasks recorded retryable and<br/>unsent deliveries released, both claimable again]

        CTRL --> BROADCAST --> DRAIN --> GIVEBACK
    end

    GIVEBACK -.-> PENDING
    GIVEBACK -.-> OUT_PENDING
```

### 3.5 Company Tasks HTMX Dashboard
- **Web Dashboard (`/companies/{id}/tasks`):** Interactive HTMX interface allowing company owners to monitor tasks, filter by workflow or status (`pending`, `processing`, `completed`, `failed`, `dead_letter`, `stopped`), and sort by time.
- **Stop / Resume Controls:** Allows manual task cancellation (dispatches a stop notification email to thread participants) or task resumption.

### 3.6 Multi-Stage Spam & Content Security Engine
- **Workflow & Participant Resolution:** Upon email arrival, the target `Company` and `Workflow` are resolved from the recipient address (`workflow-slug@company-slug.domain.com`).
- **Participant Access Control & Spam Check Bypass:** By default (`participant_emails` is `None`), channel access is restricted to Company Team members. If `participant_emails` includes `@public`, the channel is open to the public (external emails are accepted and scanned for spam). If `participant_emails` specifies a list of emails, access is restricted strictly to that list. Trusted participants (company team members or explicitly listed emails) bypass spam scan layers.
- **Stage 1 (Local Heuristic Scanner - Option B):** Fast, zero-dependency Rust-native engine analyzing email headers, subject trigger patterns (ALL CAPS, urgency keywords, excess punctuation), link shorteners (`bit.ly`, `tinyurl`), and HTML hidden text tricks (runs for public workflows).
- **Stage 2 (External `SpamScanner` - Option A):** Integrates with external spam analysis daemons via `SpamScannerService`. Supports both **Rspamd** (via HTTP API) and **SpamAssassin** (via `spamd` TCP protocol). Evaluates combined scores against `MAX_SPAM_SCORE` threshold and records metrics (`SmtpStatus::RejectedSpamScore`).
- **Stage 3 (LLM Spam Guardrail - Option C):** Pre-execution AI security check before main agent execution controlled per `Company` entity (`Company.enable_llm_spam_guardrail`) with system default fallback (`ENABLE_LLM_SPAM_GUARDRAIL=true`). Scans the prompt context using static pattern matchers and low-cost LLM classification to detect prompt injection attempts or malicious intent (runs for public workflows).

### 3.7 Multi-Agent & Multi-Recipient Handling (`To`, `CC`, and `+` Pipeline Chaining)
- **Multi-Workflow Ingestion:** Channel addresses in `To` execute normally. Channel addresses in `CC` are persisted to their thread history without executing by default; a CC'd channel executes only when the newly written body mentions its email address or a plain `@slug` matching the channel, an alias, or its assigned agent. Explicit quiet/context-only triggers still suppress execution.
- **Sequential Pipeline Chaining (`+` Syntax):** Sending to a `+`-chained recipient address (e.g., `support+billing+legal@acme.mailagents.com`) triggers sequential pipeline execution (`support` $\rightarrow$ `billing` $\rightarrow$ `legal`).
- **Cumulative Upstream Context Sharing:** In a `+` pipeline, each subsequent step agent receives the user's prompt **plus** the accumulated outputs of all prior step agents (`[Upstream Pipeline Context from Prior Step Agents]`). For example, Step 3 (`legal`) receives the outputs from both Step 1 (`support`) and Step 2 (`billing`).
- **Strict Validation, Bounce Notifications & Fuzzy Suggestions:** If an address or pipeline step is misspelled (e.g., `suppport@...` or `support+biling@...`), strict validation halts execution and the server queues an automated bounce email (`[Undeliverable] ...`) to the sender through the durable generic delivery worker, containing fuzzy suggestions calculated via Levenshtein distance matching (e.g., *"Did you mean: `support@acme.mailagents.com`?"*).
- **Isolated Thread Contexts:** An independent thread is resolved or created for each matched workflow, maintaining clean, isolated conversation history per agent.
- **Delivery Role Context (`RecipientRole`):** Each agent execution receives its recipient delivery context (`To` vs. `Cc`), injecting `[Delivery Context: Email received via TO/CC field]` into the prompt and exposing `{{ context.recipient_role }}`, `{{ context.is_to }}`, and `{{ context.is_cc }}` in system prompts.
- **Response Concatenation & Outbound Filtering:** Agent outputs from all triggered workflows are concatenated into a single consolidated email response dispatched to the human sender. Outbound replies automatically strip all agent email addresses (`*@*.<domain>`) from recipient headers (`To`/`Cc`) to prevent inter-agent reply loops while preserving human CC participants.

#### Inbound Processing & Spam Check Decision Matrix

| Channel Mode / `participant_emails` | Sender Email (`from`) | Action | Stage 1 & Stage 2 Spam Scanner | Stage 3 LLM Spam Check (Pre-Agent) |
|---|---|---|---|---|
| **Default** (`None` / empty) | Company Team Member | **Accept & Process** | **Bypassed** | **Bypassed** |
| **Default** (`None` / empty) | Non-Team Member / External | **Block / Reject Email** (`"Sender unauthorized for workflow"`) | N/A | N/A |
| **Public** (contains `@public`) | Company Team Member / Listed | **Accept & Process** | **Bypassed** | **Bypassed** |
| **Public** (contains `@public`) | Untrusted External Sender | **Scan & Process** | **Executed** (Rejects if score $\ge 5.0$) | **Executed** (Pre-execution AI check) |
| **Explicit List** (e.g. `alice@example.com`) | **IN** `participant_emails` | **Accept & Process** | **Bypassed** | **Bypassed** |
| **Explicit List** (e.g. `alice@example.com`) | **NOT in** `participant_emails` | **Block / Reject Email** (`"Sender unauthorized for workflow"`) | N/A | N/A |

### Architectural Flow

```
Inbound Email (SMTP / Webhook)
       │
       ▼
┌────────────────────────────────────────────────────────┐
│ 1. Resolve Company & Workflow                          │
│ - Match recipient email to company_slug & workflow_slug│
└──────────────────────────┬─────────────────────────────┘
                           │
             ┌─────────────┴─────────────┐
             │ Is Workflow Restricted?   │
             │ (participant_emails set?) │
             └─────────────┬─────────────┘
                YES        │        NO (Public Workflow)
                 │                  │
     ┌───────────┴───────────┐      │
     │ Is 'from' email in    │      │
     │ participant_emails?   │      │
     └─────┬───────────┬─────┘      │
        NO │           │ YES        │
           │           │            │
           ▼           │            ▼
┌──────────────────┐   │   ┌──────────────────────────────────────────────┐
│ REJECT Email     │   │   │ Stage 1: Rust-Native Heuristic Scanner       │
│ - Unauthorized   │   │   │ Stage 2: External SpamScanner (Option A)     │
│   Sender         │   │   └──────────────────────┬───────────────────────┘
└──────────────────┘   │                          │
                       │            ┌─────────────┴─────────────┐
                       │            │ `total_spam_score` >= 5.0?│
                       │            └─────────────┬─────────────┘
                       │               YES        │        NO
                       │                │                  │
                       │                ▼                  ▼
                       │     ┌──────────────────┐  ┌──────────────────┐
                       │     │ REJECT Email     │  │ Ingest Email     │
                       │     │ - Spam Threshold │  └────────┬─────────┘
                       │     └──────────────────┘           │
                       │                                    ▼
                       │                      ┌──────────────────────────┐
                       │                      │ Stage 3: LLM Spam Check  │
                       │                      │ (Pre-Agent Execution)    │
                       │                      └─────────────┬────────────┘
                       │                                    │
                       │                       ┌────────────┴────────────┐
                       │                       │ Guardrail Flagged Spam? │
                       │                       └────────────┬────────────┘
                       │                          YES       │        NO
                       │                           │                 │
                       │                           ▼                 ▼
                       │                 ┌──────────────────┐        │
                       │                 │ Cancel Execution │        │
                       │                 └──────────────────┘        │
                       │                                             │
                       ├─────────────────────────────────────────────┘
                       │ (Bypass All Spam Checks)
                       ▼
┌────────────────────────────────────────────────────────┐
│ Execute Agent Prompt & Process Thread                  │
└────────────────────────────────────────────────────────┘
```

### 3.8 Context-Only / Quiet Mode Ingestion & Reserved Slug Validation
- **Context-Only Ingestion:** Ingest emails directly into a channel thread's history without triggering agent execution or automated replies. Allows users and integrations to post background context, transcripts, or notes.
- **Trigger Suffixes & Body Commands:** Supported via recipient address subaddressing/suffixes (`channel.quiet@...`, `channel.noagent@...`, `channel.message@...`, `channel.msg@...`, `channel.na@...`, `channel+quiet@...`) or email body prefix triggers (`[[quiet]]`, `[quiet]`, `[[noagent]]`, `[noagent]`, `[[message]]`, `[msg]`, `[na]`).
- **Reserved Slug Validation:** Agent and Channel/Workflow slug creation and updates strictly validate against reserved mode keywords (`quiet`, `noagent`, `message`, `msg`, `na`) to prevent route ambiguity.
- **Thread History Integration:** Messages ingested in context-only mode are saved to database thread history. Subsequent normal messages sent to the channel trigger the agent, which reads the full thread context including all quiet notes.

### 3.9 Mailbox UI (`/ui`)
- **Three-Column Reader (daisyUI + HTMX):** `/ui` renders the company's channels as a mail-style sidebar, the selected channel's threads in the middle column (keyset pagination, "Load older threads"), and the selected thread's messages as chat bubbles on the right. Each column is swapped independently over HTMX and the selection is reflected in the URL (`/ui?company_id=…&channel_id=…&thread_id=…`), so a refresh or a shared link restores the same view.
- **Compose (New Thread):** Enabled only once a channel is selected. The composed message is fed through the normal inbound path addressed to `channel-slug@company-slug.domain` as the signed-in user, so participant rules, spam checks and agents apply unchanged. The "deliver agent reply by email" toggle selects `SimulationMode::Run` (real dispatch) over the default `RunTest` (in-app only); a rejected message re-renders the form with the channel's rejection reason.
- **Coexistence:** All existing pages (channels, agents, tasks, simulator) are untouched and reachable from the icon rail; the mailbox is an additional read/compose surface over the same use cases.

### 3.10 Credential Encryption & Rotation
- **Versioned encryption:** Provider credentials use versioned AES-256-GCM keys. `CREDENTIAL_ENCRYPTION_KEYS` defines the keys available for reads, while `CREDENTIAL_ENCRYPTION_ACTIVE_VERSION` selects the key used for new writes. The active version currently comes from deployment configuration, not PostgreSQL.
- **Explicit operations:** Startup validates the encryption configuration but never rotates database rows automatically. Operators inspect and converge rows with `credentials status` and `credentials rotate`; rotation uses authenticated encryption, bounded batches, a PostgreSQL advisory lock, and compare-and-swap updates.
- **Multi-instance rollout:** Rotate keys in separate distribute, activate, converge, retention, and retirement phases. See the [multi-machine credential-key rotation runbook](docs/deploy.md#multi-machine-credential-key-rotation) for the complete procedure.

## Development Setup

### Database Setup

To start the local PostgreSQL database:

```bash
LC_ALL="en_US.UTF-8" /opt/homebrew/opt/postgresql@16/bin/postgres -D /opt/homebrew/var/postgresql@16
```

### Database Migrations

Migrations are automatically executed on server startup via `sqlx::migrate!()`.

To run migrations manually using `psql`:

```bash
/opt/homebrew/opt/postgresql@16/bin/psql -d <database_name> -f migrations/20260817000000_init_schema.sql
```

Alternatively, install `sqlx-cli` to run migrations:

```bash
cargo install sqlx-cli --no-default-features --features postgres
sqlx migrate run
```
