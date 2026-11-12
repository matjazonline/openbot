//! The product word for a channel is desk: what a person reads says desk, while classes, ids,
//! `hx-*` targets, hrefs and form fields keep `channel` (plan 01-departments, decision 2).
use super::*;

/// Every word-bounded `channel`/`channels`, either case.
fn says_channel(text: &str) -> bool {
    static WORD: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    WORD.get_or_init(|| regex::Regex::new(r"(?i)\bchannels?\b").expect("a valid pattern"))
        .is_match(text)
}

#[test]
fn desk_wording_nav_names_the_desks_link() {
    let page = base_layout("Acme", "");
    assert!(page.contains(r##"<a id="nav-channels" href="#""##));
    assert!(page.contains(">Desks</a>"));
    assert!(!page.contains(">Channels</a>"));

    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let email = mailbox_account_email();
    let user = mailbox_user(&email);
    let mailbox = mailbox_page(&MailboxPage {
        user: &user,
        company: &company,
        companies: std::slice::from_ref(&company),
        app_domain_name: "example.com",
        channels: std::slice::from_ref(&channel),
        selected_channel: Some(&channel),
        threads: &[],
        next_cursor: None,
        selected_thread_id: None,
        activity: no_activity(),
        detail_html: "",
    });
    // The rail link keeps its route; its label, tooltip and accessible name say desk.
    assert!(mailbox.contains(&format!(
        r#"<a href="/ui/channels?company_id={}" class="btn btn-square btn-md btn-ghost" title="Desks" aria-label="Desks""#,
        company.id
    )));
    assert!(mailbox.contains(r#"<span class="rail-label">Desks</span>"#));
    assert!(mailbox.contains("Inbound desks and conversations."));
    assert!(mailbox.contains("Edit Desk</a>"));
    assert!(mailbox.contains("New Desk</a>"));
    assert!(mailbox.contains(r#"title="Start a new thread in this desk""#));
}

#[test]
fn desk_wording_desks_page_heading() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let html = channels_page(&ChannelsPage {
        departments: &[],
        company: &company,
        app_domain_name: "example.com",
        channels: std::slice::from_ref(&channel),
        spam_scan_enabled: false,
        focus: ChannelsPageFocus::default(),
    });

    assert!(html.contains("<title>Acme Desks - Mail Agents</title>"));
    assert!(html.contains(r#"<h2 class="text-2xl font-bold text-base-content">Acme Desks</h2>"#));
    assert!(html.contains("Manage desks for"));
    assert!(html.contains(
        r#"<h3 class="text-sm font-semibold uppercase tracking-wider text-base-content/70 mb-3">Desks</h3>"#
    ));
    assert!(html.contains("Add New Desk"));
    assert!(html.contains(">Desk Name</label>"));
    assert!(html.contains("Desks without participant email restrictions"));
    assert!(html.contains(r#"hx-confirm="Are you sure you want to delete desk 'Inbox'?""#));
    // Identifiers are not copy.
    assert!(html.contains(r#"id="channel-list""#));
    assert!(html.contains(r#"hx-post="/companies/"#));

    let empty = channel_list_fragment(&company, "example.com", &[], &[]);
    assert!(empty.contains("No desks configured yet. Use Add Desk to create your first one."));
}

#[test]
fn desk_wording_settings_breadcrumb() {
    let company = mailbox_company();
    let email = mailbox_account_email();
    let user = mailbox_user(&email);
    let html = channel_settings_page(&ChannelSettingsPage {
        user: &user,
        companies: std::slice::from_ref(&company),
        list: &ChannelSettingsList {
            departments: &[],
            company: &company,
            app_domain_name: "example.com",
            channels: &[],
            selected_channel_id: None,
        },
        pane_html: "",
    });

    assert!(html.contains("<title>Acme Desks - Mail Agents</title>"));
    assert!(html.contains(
        r##"<h2 class="min-w-0 flex-1 text-base font-semibold leading-tight">Desks</h2>"##
    ));
    assert!(html.contains("New Desk</button>"));
    assert!(html.contains("No desks yet. Create your first one below."));
    assert!(!html.contains(">Channels<"));
}

#[test]
fn desk_wording_company_workspace_count() {
    let company = mailbox_company();
    let html = company_edit_pane(&CompanyEditPane {
        company: &company,
        model_connections: &[],
        app_domain_name: "example.com",
        counts: CompanyCounts {
            departments: 0,
            channels: 7,
            agents: 2,
            skills: 0,
        },
        draft: None,
        error: None,
        editable: true,
        resend_api_section: "",
        body: CompanyPaneBody::Settings,
    });

    let link = format!(r#"<a href="/ui/channels?company_id={}""#, company.id);
    let start = html
        .find(&link)
        .expect("the desks count links to the desks workspace");
    let end = start + html[start..].find("</a>").expect("the link closes");
    let count = &html[start..end];
    assert!(
        count.contains(r#"<span class="text-lg font-bold">7</span>"#),
        "{count}"
    );
    assert!(count.contains(r#"<span class="text-[11px] font-normal opacity-60">Desks</span>"#));
    assert!(html.contains("Its desks, agents and threads go with it."));
}

#[test]
fn desk_wording_flash_messages_say_desk() {
    let company = mailbox_company();
    let agent = settings_agent(company.id, "Support", "support");

    let created = assistant_channel_card(
        company.id,
        &agent,
        Some(AssistantChannelOutcome::Created {
            channel: "support-assistant",
            address: "support-assistant@acme.example.com",
        }),
    );
    assert!(created.contains(
        r#"<div role="status" class="alert alert-success text-sm">Created the team desk support-assistant (support-assistant@acme.example.com); it now answers through its workflow.</div>"#
    ));
    // The agent already has its own desk (01-departments 04B.3): this one is additional.
    assert!(created.contains(r#"<h3 class="font-semibold">Additional assistant desk</h3>"#));
    assert!(created.contains("Create additional desk</button>"));
    // The form still posts to the channel route with channel field names.
    assert!(created.contains(r#"name="channel_name""#));
    assert!(created.contains(r#"name="channel_slug""#));

    let email = mailbox_account_email();
    let user = mailbox_user(&email);
    let triage = settings_agent(company.id, "Triage", "triage");
    let missing = onboarding_complete_page(
        &user,
        &company,
        &[OnboardedAgent {
            agent: &triage,
            channel: OnboardingChannel::Missing,
        }],
    );
    assert!(missing.contains("No desk yet."));
    assert!(missing.contains("Create an assistant desk</a>"));
}

/// Collect the value of every string literal in `source`, except doc comments: the copy a page
/// can render lives in those literals, and nowhere else in the file.
fn string_literals(source: &str) -> Vec<String> {
    fn walk(mut cursor: syn::buffer::Cursor<'_>, out: &mut Vec<String>) {
        while !cursor.eof() {
            if let Some((punct, rest)) = cursor.punct()
                && punct.as_char() == '#'
            {
                let rest = rest
                    .punct()
                    .filter(|(inner, _)| inner.as_char() == '!')
                    .map_or(rest, |(_, after)| after);
                if let Some((inside, _, _, after)) = rest.any_group()
                    && inside.ident().is_some_and(|(ident, _)| ident == "doc")
                {
                    cursor = after;
                    continue;
                }
                cursor = rest;
            } else if let Some((literal, rest)) = cursor.literal() {
                if let syn::Lit::Str(text) = syn::Lit::new(literal) {
                    out.push(text.value());
                }
                cursor = rest;
            } else if let Some((inside, _, _, rest)) = cursor.any_group() {
                walk(inside, out);
                cursor = rest;
            } else if let Some((_, rest)) = cursor.token_tree() {
                cursor = rest;
            } else {
                break;
            }
        }
    }
    use syn::parse::Parser;
    let parser = |input: syn::parse::ParseStream<'_>| {
        input.step(|cursor| {
            let mut out = Vec::new();
            walk(*cursor, &mut out);
            let mut end = *cursor;
            while let Some((_, rest)) = end.token_tree() {
                end = rest;
            }
            Ok((out, end))
        })
    };
    parser.parse_str(source).expect("a page source tokenizes")
}

/// What of a literal a reader can see: `None` for a script const (its `>`/`<` are operators and
/// its words are code), otherwise the literal without inline `<script>` blocks and with `format!`
/// placeholders such as `{channel}` removed, since those name an argument rather than say a word.
fn markup_of(literal: &str) -> Option<String> {
    static SCRIPT: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static PLACEHOLDER: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    if literal.contains("function ") {
        return None;
    }
    let script = SCRIPT.get_or_init(|| regex::Regex::new(r"(?is)<script\b.*?</script>").unwrap());
    let placeholder = PLACEHOLDER.get_or_init(|| regex::Regex::new(r"\{[^{}]*\}").unwrap());
    let markup = script.replace_all(literal, "");
    Some(placeholder.replace_all(&markup, "").into_owned())
}

/// Every page source under `pages/`, test modules excluded: they quote the old word on purpose.
fn page_sources() -> Vec<(std::path::PathBuf, String)> {
    fn visit(dir: &std::path::Path, out: &mut Vec<(std::path::PathBuf, String)>) {
        for entry in std::fs::read_dir(dir).expect("a pages directory") {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                visit(&path, out);
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !name.ends_with(".rs") || name == "tests.rs" || name.ends_with("_tests.rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("a page source");
            out.push((path, source));
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/adapters/http/pages");
    let mut out = Vec::new();
    visit(&root, &mut out);
    out
}

#[test]
fn desk_wording_page_sources_keep_channel_out_of_text_nodes() {
    static TEXT_NODE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static VISIBLE_ATTRIBUTE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let text_node = TEXT_NODE.get_or_init(|| regex::Regex::new(r">([^<>]*)<").unwrap());
    let visible_attribute = VISIBLE_ATTRIBUTE.get_or_init(|| {
        regex::Regex::new(r#"\b(?:title|aria-label|placeholder|alt|hx-confirm)="([^"]*)""#).unwrap()
    });

    let sources = page_sources();
    // A path that silently reads nothing would pass every assertion below.
    assert!(
        sources.len() > 40,
        "only {} page sources were read",
        sources.len()
    );

    let mut found = Vec::new();
    for (path, source) in &sources {
        // The literal text-node forms, anywhere in the file.
        for pattern in [
            ">Channel",
            "Channel<",
            ">Channels",
            "Channels<",
            ">channel",
            "channel<",
            "channels<",
        ] {
            if source.contains(pattern) {
                found.push(format!("{}: `{pattern}`", path.display()));
            }
        }
        // Text nodes and user-visible attributes inside the string literals that build the page,
        // whatever whitespace or placeholders surround the word.
        for literal in string_literals(source) {
            let Some(markup) = markup_of(&literal) else {
                continue;
            };
            for capture in text_node.captures_iter(&markup) {
                if says_channel(&capture[1]) {
                    found.push(format!("{}: text {:?}", path.display(), capture[1].trim()));
                }
            }
            for capture in visible_attribute.captures_iter(&markup) {
                if says_channel(&capture[1]) {
                    found.push(format!("{}: attribute {:?}", path.display(), &capture[1]));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "the product word is desk, not channel:\n{}",
        found.join("\n")
    );
}

/// The scan above is only as good as the literals it sees: prove it finds the forms it guards and
/// passes over code, comments and placeholders.
#[test]
fn desk_wording_scan_reads_text_nodes_in_every_literal_form() {
    let source = r####"
        /// A doc comment saying <b>Channel</b> is not copy.
        const A: &str = r##"<a href="/ui/channels">
            Channels
        </a>"##;
        fn b() -> String { format!("<span>{} New channel</span>", 1) }
        fn c() -> String { format!("<b>{channel}</b><i title=\"Open the channel\"></i>") }
        const SCRIPT: &str = "function f(a) { return a > 1 && channel < 2; }";
        // A line comment saying <b>Channel</b> is not copy either.
    "####;
    let literals = string_literals(source);
    assert_eq!(literals.len(), 4, "{literals:?}");
    assert!(!literals.iter().any(|literal| literal.contains("comment")));
    let text_node = regex::Regex::new(r">([^<>]*)<").unwrap();
    let attribute = regex::Regex::new(r#"\btitle="([^"]*)""#).unwrap();
    let markup: Vec<_> = literals
        .iter()
        .filter_map(|literal| markup_of(literal))
        .collect();
    assert_eq!(markup.len(), 3, "the script const is not markup");
    let flagged: Vec<_> = markup
        .iter()
        .flat_map(|literal| text_node.captures_iter(literal))
        .chain(
            markup
                .iter()
                .flat_map(|literal| attribute.captures_iter(literal)),
        )
        .filter(|capture| says_channel(&capture[1]))
        .map(|capture| capture[1].trim().to_owned())
        .collect();
    assert_eq!(flagged, ["Channels", "New channel", "Open the channel"]);
}
