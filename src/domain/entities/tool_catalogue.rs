//! The library of tools an agent may be granted.
//!
//! It is a compile-time const rather than a table because there is nothing to store: the
//! `ai-agents` built-ins are a hardcoded array upstream, and our four native tools are consts in
//! `src/application/services/*_tool.rs`. A table would only be a slower copy of this file that an
//! operator could edit into granting `command`.
//!
//! Two things live here and are deliberately kept apart: [`ALLOWED_BUILTIN_TOOL_IDS`], which is a
//! safety boundary, and [`TOOL_CATALOGUE`], which is that boundary plus the copy a picker needs.

use crate::entities::value_objects::ToolId;

/// The native tool ids, owned here so the catalogue and the tool implementations cannot name the
/// same tool differently. `src/application/services/*_tool.rs` re-export these rather than
/// repeating the literals.
pub const OUTREACH_TOOL_ID: &str = "outreach_and_await_quorum";
pub const CREATE_AGENT_CHANNEL_TOOL_ID: &str = "create_agent_channel";
pub const AGENT_DIRECTORY_TOOL_ID: &str = "list_company_agents";
pub const TASK_OWNERSHIP_TOOL_ID: &str = "transfer_or_release_task";
pub const REQUEST_APPROVAL_TOOL_ID: &str = "request_approval";

/// Every `ai-agents` built-in this platform will ever put in a `tools:` list.
///
/// This is an allowlist and not a blocklist on purpose: the upstream `BUILTIN_TOOL_IDS` is a
/// hardcoded 30-entry array that grows when the pinned revision moves, and a blocklist would grant
/// each new arrival by default. There is no environment override and no per-company escape -- the
/// same list applies to every agent on the platform.
///
/// The absentees are absent because they execute in this process, on this host, with no sandbox,
/// and inbound mail is an untrusted prompt source: `command` (arbitrary execution), the mutation
/// family `file_write` / `file_edit` / `patch` / `copy_path` / `move_path` / `delete_path`, the
/// read family `file` / `file_read` / `file_list` / `file_info` / `glob` / `grep`, the repository
/// pair `git_status` / `git_diff`, `diagnostics`, `sleep` (an unbounded wall-clock hold inside a
/// leased task), and `ask_user` (blocks for a terminal operator who does not exist in a mail
/// server). Revisit this list when -- and only when -- a sandboxed harness exists to run them in.
///
/// `http` is excluded too: unlike `web_fetch`, the pinned implementation accepts arbitrary URLs,
/// redirects, methods and headers without URL/DNS/IP or response-size checks. In this unsandboxed
/// process that is an SSRF and resource-exhaustion primitive. `web_search` is host-dependent and
/// has no provider here; allowing it would leave a latent network grant that silently becomes live
/// if one is installed later, without `web_fetch`'s URL/DNS/IP protections.
///
/// The drift test that proves each of these is still a built-in upstream lives in the harness
/// adapter, where importing `ai_agents` is legitimate -- `src/domain/` must not.
pub const ALLOWED_BUILTIN_TOOL_IDS: [&str; 10] = [
    "calculator",
    "datetime",
    "echo",
    "json",
    "math",
    "random",
    "template",
    "text",
    "todo",
    "web_fetch",
];

/// Who implements a tool, which is also who is responsible for what it may reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolSource {
    /// Ships with the `ai-agents` runtime; registered by its `auto_configure_features()`.
    Builtin,
    /// Ours, implemented in `src/application/services/*_tool.rs`.
    Native,
}

impl ToolSource {
    /// The heading a picker groups this source under.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Builtin => "Built-in tools",
            Self::Native => "Company tools",
        }
    }
}

/// One grantable tool, with the copy a picker shows beside its checkbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogueTool {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub source: ToolSource,
}

/// Every tool an agent may be granted, in the order a picker offers them: built-ins first, in the
/// order of [`ALLOWED_BUILTIN_TOOL_IDS`], then ours.
pub const TOOL_CATALOGUE: &[CatalogueTool] = &[
    CatalogueTool {
        id: "calculator",
        label: "Calculator",
        description: "Evaluate an arithmetic expression, with parentheses and exponentiation.",
        source: ToolSource::Builtin,
    },
    CatalogueTool {
        id: "datetime",
        label: "Date and time",
        description: "Read the current UTC time, and format, parse, shift and difference dates.",
        source: ToolSource::Builtin,
    },
    CatalogueTool {
        id: "echo",
        label: "Echo",
        description: "Return the input unchanged. Useful for checking that a skill step runs.",
        source: ToolSource::Builtin,
    },
    CatalogueTool {
        id: "json",
        label: "JSON",
        description: "Parse, query, merge and stringify JSON, and read an object's keys or values.",
        source: ToolSource::Builtin,
    },
    CatalogueTool {
        id: "math",
        label: "Statistics",
        description: "Summarise numbers: mean, median, mode, standard deviation, sum, min and max.",
        source: ToolSource::Builtin,
    },
    CatalogueTool {
        id: "random",
        label: "Random values",
        description: "Generate a UUID, a number, a coin flip, or pick from and shuffle a list.",
        source: ToolSource::Builtin,
    },
    CatalogueTool {
        id: "template",
        label: "Templates",
        description: "Render a Jinja-style template with variables, filters, conditions and loops.",
        source: ToolSource::Builtin,
    },
    CatalogueTool {
        id: "text",
        label: "Text",
        description: "Unicode-aware string work: slice, case, trim, replace, split, join and pad.",
        source: ToolSource::Builtin,
    },
    CatalogueTool {
        id: "todo",
        label: "Task list",
        description: "Keep a structured checklist for the length of one run. Nothing is persisted.",
        source: ToolSource::Builtin,
    },
    CatalogueTool {
        id: "web_fetch",
        label: "Fetch a web page",
        description: "Read one public URL, with redirect, address and response-size limits applied.",
        source: ToolSource::Builtin,
    },
    CatalogueTool {
        id: OUTREACH_TOOL_ID,
        label: "Contact people and await replies",
        description: "Mail one or more recipients and suspend the task until enough of them \
                      answer, or until a human decides. Recipients may be colleagues or other \
                      agents in this company.",
        source: ToolSource::Native,
    },
    CatalogueTool {
        id: AGENT_DIRECTORY_TOOL_ID,
        label: "List this company's agents",
        description: "Look up the other agents in this company, what each does, and the address \
                      to reach it on.",
        source: ToolSource::Native,
    },
    CatalogueTool {
        id: CREATE_AGENT_CHANNEL_TOOL_ID,
        label: "Create a specialist agent",
        description: "Permanently create another agent in this company, with its own channel \
                      address, and delegate to it.",
        source: ToolSource::Native,
    },
    CatalogueTool {
        id: TASK_OWNERSHIP_TOOL_ID,
        label: "Transfer or release this task",
        description: "Transfer owned work with a private handoff, or release it unassigned and end the current run.",
        source: ToolSource::Native,
    },
    CatalogueTool {
        id: REQUEST_APPROVAL_TOOL_ID,
        label: "Request human approval",
        description: "Present a concrete proposal and wait for an explicit human checkpoint decision.",
        source: ToolSource::Native,
    },
];

impl CatalogueTool {
    /// Capability support is independent of per-run context. Checkpoints remain unavailable in
    /// ai-agents until that adapter proves stable invocation replay and full-context suspension.
    pub fn supports_harness(&self, harness: super::harness::HarnessKind) -> bool {
        match harness {
            super::harness::HarnessKind::AiAgents => self.id != REQUEST_APPROVAL_TOOL_ID,
            super::harness::HarnessKind::Rig => {
                ALLOWED_BUILTIN_TOOL_IDS.contains(&self.id)
                    || matches!(
                        self.id,
                        OUTREACH_TOOL_ID
                            | AGENT_DIRECTORY_TOOL_ID
                            | CREATE_AGENT_CHANNEL_TOOL_ID
                            | TASK_OWNERSHIP_TOOL_ID
                            | REQUEST_APPROVAL_TOOL_ID
                    )
            }
        }
    }
    /// The catalogue entry for `id`, or `None` when nothing by that name may be granted.
    ///
    /// This is the only validity check a [`ToolId`] has, which is why the type has no `parse`.
    pub fn get(id: &ToolId) -> Option<&'static CatalogueTool> {
        TOOL_CATALOGUE.iter().find(|tool| tool.id == id.as_str())
    }

    /// Every grantable tool, in stable picker order and already grouped by source.
    pub fn grantable() -> impl Iterator<Item = &'static CatalogueTool> {
        TOOL_CATALOGUE.iter()
    }
}

/// What survived a grant list and what did not.
///
/// `refused` is returned rather than silently dropped so the caller can `warn!` each id with the
/// agent it belonged to: a capability that vanishes without a log is a support ticket nobody can
/// answer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GrantFilter {
    pub granted: Vec<ToolId>,
    pub refused: Vec<ToolId>,
}

/// Split a stored grant list into the ids that are in the catalogue and the ids that are not,
/// preserving order and dropping duplicates.
///
/// Pure and synchronous on purpose: this is what the harness compiler calls on the agent-run path,
/// where an `async fn` would cost a stack frame the chain does not have to spare.
pub fn retain_grantable(ids: &[ToolId]) -> GrantFilter {
    let mut filter = GrantFilter::default();
    for id in ids {
        let bucket = if CatalogueTool::get(id).is_some() {
            &mut filter.granted
        } else {
            &mut filter.refused
        };
        if !bucket.contains(id) {
            bucket.push(id.clone());
        }
    }
    filter
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every built-in `ai-agents` id the pinned revision ships, so the exclusions can be asserted
    /// by name rather than by absence from a list this module also writes.
    const UPSTREAM_BUILTIN_TOOL_IDS: [&str; 30] = [
        "calculator",
        "echo",
        "datetime",
        "json",
        "random",
        "file",
        "glob",
        "grep",
        "file_read",
        "file_write",
        "file_edit",
        "patch",
        "copy_path",
        "move_path",
        "delete_path",
        "file_list",
        "file_info",
        "git_status",
        "git_diff",
        "diagnostics",
        "ask_user",
        "todo",
        "sleep",
        "web_fetch",
        "web_search",
        "command",
        "text",
        "template",
        "math",
        "http",
    ];

    #[test]
    fn retain_grantable_refuses_every_host_access_builtin() {
        let excluded: Vec<ToolId> = UPSTREAM_BUILTIN_TOOL_IDS
            .into_iter()
            .filter(|id| !ALLOWED_BUILTIN_TOOL_IDS.contains(id))
            .map(ToolId::from)
            .collect();

        assert_eq!(
            excluded.len(),
            20,
            "the allowlist admits 10 of 30 built-ins"
        );

        let filter = retain_grantable(&excluded);
        assert!(filter.granted.is_empty(), "granted {:?}", filter.granted);
        assert_eq!(filter.refused, excluded);
        assert!(filter.refused.contains(&ToolId::from("command")));
    }

    #[test]
    fn retain_grantable_keeps_allowlisted_and_native_ids() {
        let ids = [
            ToolId::from("datetime"),
            ToolId::from("command"),
            ToolId::from(OUTREACH_TOOL_ID),
        ];

        let filter = retain_grantable(&ids);

        assert_eq!(
            filter.granted,
            [ToolId::from("datetime"), ToolId::from(OUTREACH_TOOL_ID)]
        );
        assert_eq!(filter.refused, [ToolId::from("command")]);
    }

    #[test]
    fn retain_grantable_drops_duplicates_and_keeps_first_use_order() {
        let ids = [
            ToolId::from("json"),
            ToolId::from("datetime"),
            ToolId::from("json"),
            ToolId::from("command"),
            ToolId::from("command"),
        ];

        let filter = retain_grantable(&ids);

        assert_eq!(
            filter.granted,
            [ToolId::from("json"), ToolId::from("datetime")]
        );
        assert_eq!(filter.refused, [ToolId::from("command")]);
    }

    #[test]
    fn an_unknown_tool_id_is_not_in_the_catalogue() {
        assert!(CatalogueTool::get(&ToolId::from("no_such_tool")).is_none());
        assert!(CatalogueTool::get(&ToolId::from("")).is_none());
    }

    #[test]
    fn the_catalogue_lists_exactly_the_allowlisted_builtins() {
        let catalogued: Vec<&str> = CatalogueTool::grantable()
            .filter(|tool| tool.source == ToolSource::Builtin)
            .map(|tool| tool.id)
            .collect();

        assert_eq!(catalogued, ALLOWED_BUILTIN_TOOL_IDS);
    }

    #[test]
    fn the_catalogue_has_no_duplicate_ids_and_no_blank_copy() {
        let mut seen: Vec<&str> = Vec::new();
        for tool in CatalogueTool::grantable() {
            assert!(!seen.contains(&tool.id), "{} appears twice", tool.id);
            assert!(!tool.label.trim().is_empty(), "{} has no label", tool.id);
            assert!(
                !tool.description.trim().is_empty(),
                "{} has no description",
                tool.id
            );
            seen.push(tool.id);
        }
        assert_eq!(seen.len(), ALLOWED_BUILTIN_TOOL_IDS.len() + 5);
    }

    #[test]
    fn the_native_tools_are_grantable_and_grouped_last() {
        let sources: Vec<ToolSource> = CatalogueTool::grantable().map(|tool| tool.source).collect();
        let first_native = sources
            .iter()
            .position(|source| *source == ToolSource::Native)
            .expect("the catalogue lists our native tools");
        assert!(
            sources[first_native..]
                .iter()
                .all(|source| *source == ToolSource::Native)
        );

        for id in [
            OUTREACH_TOOL_ID,
            AGENT_DIRECTORY_TOOL_ID,
            CREATE_AGENT_CHANNEL_TOOL_ID,
            TASK_OWNERSHIP_TOOL_ID,
            REQUEST_APPROVAL_TOOL_ID,
        ] {
            assert!(
                CatalogueTool::get(&ToolId::from(id)).is_some(),
                "{id} is missing from the catalogue"
            );
        }
    }
}
