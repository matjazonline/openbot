use crate::entities::response_draft::{
    DraftRecipientSnapshot, DraftTransportSnapshot, EvidenceSource, ResponseReviewDetail,
};
use crate::use_cases::response_review::ResponseReviewPolicy;

use super::{base_layout, escape_html_attr, escape_html_text};

pub fn response_review_list_page(
    details: &[ResponseReviewDetail],
    company_id: uuid::Uuid,
) -> String {
    let items = if details.is_empty() {
        r#"<p class="text-slate-400">No responses are waiting for your review.</p>"#.into()
    } else {
        details
            .iter()
            .map(|detail| {
                format!(
                    r#"<li class="rounded-xl border border-slate-700 p-4">
                        <a class="text-indigo-300 hover:text-white font-semibold" href="/reviews/{id}?company_id={company_id}">{subject}</a>
                        <p class="mt-1 text-sm text-slate-400">Version {version} · expires {expires}</p>
                    </li>"#,
                    id = detail.draft.id,
                    subject = escape_html_text(&detail.draft.subject),
                    version = detail.draft.version,
                    expires = detail.review.expires_at.format("%Y-%m-%d %H:%M UTC"),
                )
            })
            .collect::<Vec<_>>()
            .join("")
    };
    base_layout(
        "Response reviews",
        &format!(
            r#"<header class="mb-6"><h1 class="text-2xl font-bold">Response reviews</h1>
                <p class="text-slate-400">Pending responses assigned to you.</p></header>
                <ul class="space-y-3">{items}</ul>"#
        ),
    )
}

pub fn response_review_detail_page(detail: &ResponseReviewDetail) -> String {
    let (edit_to, edit_cc) = match &detail.draft.recipients {
        DraftRecipientSnapshot::V1 { to, cc } => (
            to.first()
                .map(|value| value.as_str().to_string())
                .unwrap_or_default(),
            cc.iter()
                .map(|value| value.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        ),
    };
    let recipients = match &detail.draft.recipients {
        DraftRecipientSnapshot::V1 { to, cc } => {
            let to = to
                .iter()
                .map(|value| escape_html_text(value.as_str()))
                .collect::<Vec<_>>()
                .join(", ");
            let cc = cc
                .iter()
                .map(|value| escape_html_text(value.as_str()))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "<p><span class=\"text-slate-400\">To:</span> {to}</p><p><span class=\"text-slate-400\">Cc:</span> {cc}</p>"
            )
        }
    };
    let destination = match &detail.draft.transport {
        DraftTransportSnapshot::V1 {
            transport,
            external_destination,
            destination_binding_id,
            ..
        } => external_destination
            .as_ref()
            .map(|value| value.as_str().to_string())
            .unwrap_or_else(|| format!("{} binding {}", transport.label(), destination_binding_id)),
    };
    let evidence = if detail.evidence.is_empty() {
        "<li class=\"text-slate-400\">No evidence references were retained.</li>".into()
    } else {
        detail.evidence.iter().map(|item| {
            let source = evidence_label(&item.evidence.source);
            let availability = if item.openable { "Available" } else { "Unavailable" };
            let warning = item.unavailable_reason.as_deref().map(escape_html_text).unwrap_or_default();
            format!(r#"<li class="rounded-lg border border-slate-700 p-3">
                <p>{source} <span class="text-xs text-slate-400">{support} · {availability}</span></p>
                <p class="text-xs text-amber-300">{warning}</p></li>"#,
                source = escape_html_text(&source),
                support = escape_html_text(item.evidence.support.as_str()),
            )
        }).collect::<Vec<_>>().join("")
    };
    let warnings = detail
        .visibility_warnings
        .iter()
        .map(|warning| format!("<li>{}</li>", escape_html_text(warning)))
        .collect::<Vec<_>>()
        .join("");
    let history = detail
        .history
        .iter()
        .map(|draft| {
            format!(
                "<li>Version {} · {}</li>",
                draft.version,
                escape_html_text(draft.status.as_str())
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let attachments = if detail.draft.attachments.is_empty() {
        "<li class=\"text-slate-400\">No attachments.</li>".into()
    } else {
        detail
            .draft
            .attachments
            .iter()
            .map(|attachment| {
                format!(
                    "<li>{} <span class=\"text-xs text-slate-400\">{}</span></li>",
                    escape_html_text(&attachment.filename),
                    escape_html_text(&attachment.sha256_hash),
                )
            })
            .collect::<Vec<_>>()
            .join("")
    };
    let decision = match (
        detail.review.feedback.as_deref(),
        detail.review.reviewer_rationale.as_deref(),
    ) {
        (Some(feedback), _) => format!(
            "<p class=\"my-4 rounded-lg border border-rose-800 p-3\"><strong>Feedback:</strong> {}</p>",
            escape_html_text(feedback),
        ),
        (_, Some(rationale)) => format!(
            "<p class=\"my-4 rounded-lg border border-slate-700 p-3\"><strong>Decision rationale:</strong> {}</p>",
            escape_html_text(rationale),
        ),
        _ => String::new(),
    };
    let common = format!(
        r#"<input type="hidden" name="company_id" value="{company_id}">
            <input type="hidden" name="draft_id" value="{draft_id}">
            <input type="hidden" name="expected_draft_version" value="{version}">"#,
        company_id = detail.draft.company_id,
        draft_id = detail.draft.id,
        version = detail.draft.version,
    );
    let content = format!(
        r##"<a class="text-sm text-indigo-300" href="/reviews?company_id={company_id}">← All reviews</a>
        <a class="ml-4 text-sm text-indigo-300" href="/reviews/policy?company_id={company_id}&amp;channel_id={channel_id}">Review policy</a>
        <header class="my-5"><h1 class="text-2xl font-bold">{subject}</h1>
            <p class="text-slate-400">Draft version {version} · {status}</p></header>
        <section class="space-y-1 rounded-xl border border-slate-700 p-4">{recipients}
            <p><span class="text-slate-400">Destination:</span> {destination}</p></section>
        <section class="my-5"><h2 class="font-semibold mb-2">Exact draft</h2>
            <pre class="whitespace-pre-wrap rounded-xl bg-slate-950 p-4 text-sm">{body}</pre></section>
        <section class="my-5"><h2 class="font-semibold mb-2">Attachments</h2><ul>{attachments}</ul></section>
        <section class="my-5"><h2 class="font-semibold mb-2">Evidence</h2><ul class="space-y-2">{evidence}</ul></section>
        <ul class="my-4 text-sm text-amber-300">{warnings}</ul>
        {decision}
        <section class="my-5"><h2 class="font-semibold mb-2">Version history</h2><ul class="text-sm text-slate-400">{history}</ul></section>
        <div id="review-result"></div>
        <form method="post" action="/reviews/edit" hx-post="/reviews/edit" hx-target="#review-result" hx-disabled-elt="find button[type='submit']" class="my-5 space-y-2 rounded-xl border border-indigo-800 p-4">
          {common}<input type="hidden" name="command_id" value="{edit_command}">
          <h2 class="font-semibold">Edit as a new version</h2>
          <label class="block text-sm">Subject</label>
          <input class="w-full rounded-lg bg-slate-950 p-2" name="subject" value="{edit_subject}" required>
          <label class="block text-sm">To</label>
          <input class="w-full rounded-lg bg-slate-950 p-2" name="recipient_to" value="{edit_to}" required>
          <label class="block text-sm">Cc (comma separated)</label>
          <input class="w-full rounded-lg bg-slate-950 p-2" name="recipients_cc" value="{edit_cc}">
          <label class="block text-sm">Response</label>
          <textarea class="w-full rounded-lg bg-slate-950 p-2" name="body" required>{edit_body}</textarea>
          <button class="rounded-lg bg-indigo-700 px-4 py-2 font-semibold" type="submit">Save new version</button>
        </form>
        <div class="grid gap-4 md:grid-cols-2">
          <form method="post" action="/reviews/approve" hx-post="/reviews/approve" hx-target="#review-result" hx-disabled-elt="find button[type='submit']" class="space-y-2 rounded-xl border border-emerald-800 p-4">
            {common}<input type="hidden" name="command_id" value="{approve_command}">
            <label class="block text-sm">Decision rationale (optional, not model reasoning)</label>
            <textarea class="w-full rounded-lg bg-slate-950 p-2" name="rationale" maxlength="2048"></textarea>
            <button class="rounded-lg bg-emerald-700 px-4 py-2 font-semibold" type="submit">Approve and publish</button>
          </form>
          <form method="post" action="/reviews/reject" hx-post="/reviews/reject" hx-target="#review-result" hx-disabled-elt="find button[type='submit']" class="space-y-2 rounded-xl border border-rose-900 p-4">
            {common}<input type="hidden" name="command_id" value="{reject_command}">
            <label class="block text-sm">Feedback</label>
            <textarea class="w-full rounded-lg bg-slate-950 p-2" name="feedback" maxlength="8192" required></textarea>
            <button class="rounded-lg bg-rose-800 px-4 py-2 font-semibold" type="submit">Reject</button>
          </form>
          <form method="post" action="/reviews/reassign" hx-post="/reviews/reassign" hx-target="#review-result" hx-disabled-elt="find button[type='submit']" class="space-y-2 rounded-xl border border-slate-700 p-4">
            {common}<input type="hidden" name="command_id" value="{reassign_command}">
            <label class="block text-sm">New reviewer principal ID</label>
            <input class="w-full rounded-lg bg-slate-950 p-2" name="reviewer_principal_id" required>
            <button class="rounded-lg bg-slate-700 px-4 py-2 font-semibold" type="submit">Reassign</button>
          </form>
        </div>"##,
        company_id = detail.draft.company_id,
        channel_id = detail.draft.channel_id,
        subject = escape_html_text(&detail.draft.subject),
        version = detail.draft.version,
        status = escape_html_text(detail.review.status.as_str()),
        destination = escape_html_text(&destination),
        body = escape_html_text(&detail.draft.body),
        approve_command = uuid::Uuid::new_v4(),
        reject_command = uuid::Uuid::new_v4(),
        reassign_command = uuid::Uuid::new_v4(),
        edit_command = uuid::Uuid::new_v4(),
        edit_subject = escape_html_attr(&detail.draft.subject),
        edit_to = escape_html_attr(&edit_to),
        edit_cc = escape_html_attr(&edit_cc),
        edit_body = escape_html_text(&detail.draft.body),
        common = common,
        attachments = attachments,
        decision = decision,
    );
    base_layout("Review response", &content)
}

fn evidence_label(source: &EvidenceSource) -> String {
    match source {
        EvidenceSource::Message {
            message_id,
            thread_id,
        } => format!("Message {message_id} in thread {thread_id}"),
        EvidenceSource::Note { note_id } => format!("Internal note {note_id}"),
        EvidenceSource::Attachment {
            message_id,
            sha256_hash,
        } => format!("Attachment on {message_id} ({sha256_hash})"),
        EvidenceSource::DelegatedResult {
            task_id,
            execution_generation,
        } => format!("Delegated result {task_id}/{execution_generation}"),
        EvidenceSource::RetainedToolResult { task_id, result_id } => {
            format!("Retained tool result {task_id}/{result_id}")
        }
        EvidenceSource::ExternalUrl { url, retrieved_at } => {
            format!("URL {url} retrieved {retrieved_at}")
        }
    }
}

pub fn response_review_action_result(message: &str, company_id: uuid::Uuid) -> String {
    format!(
        r#"<div class="rounded-xl border border-emerald-700 bg-emerald-950/60 p-4 text-emerald-200">{}
            <a class="ml-2 underline" href="/reviews?company_id={}">Back to reviews</a></div>"#,
        escape_html_text(message),
        escape_html_attr(&company_id.to_string()),
    )
}

pub fn response_review_policy_page(
    company_id: uuid::Uuid,
    channel_id: uuid::Uuid,
    policy: &ResponseReviewPolicy,
) -> String {
    let company_autonomous = selected(policy.company_default.as_str() == "autonomous");
    let company_review = selected(policy.company_default.as_str() == "review_all_external");
    let inherit = selected(policy.channel_override.is_none());
    let channel_autonomous = selected(
        policy
            .channel_override
            .is_some_and(|value| value.as_str() == "autonomous"),
    );
    let channel_review = selected(
        policy
            .channel_override
            .is_some_and(|value| value.as_str() == "review_all_external"),
    );
    let preferred = policy
        .preferred_reviewer_principal_id
        .map(|value| value.to_string())
        .unwrap_or_default();
    let body = format!(
        r#"<header class="mb-6"><h1 class="text-2xl font-bold">Response review policy</h1>
           <p class="text-slate-400">Effective policy: {effective}</p></header>
           <div id="policy-result"></div>
           <form method="post" action="/reviews/policy/company" class="mb-5 space-y-3 rounded-xl border border-slate-700 p-4">
             <input type="hidden" name="company_id" value="{company_id}">
             <input type="hidden" name="channel_id" value="{channel_id}">
             <label class="block font-semibold">Company default</label>
             <select name="policy" class="rounded-lg bg-slate-950 p-2">
               <option value="autonomous" {company_autonomous}>Autonomous</option>
               <option value="review_all_external" {company_review}>Review all external responses</option>
             </select>
             <button class="rounded-lg bg-indigo-700 px-4 py-2" type="submit">Save company default</button>
           </form>
           <form method="post" action="/reviews/policy/channel" class="space-y-3 rounded-xl border border-slate-700 p-4">
             <input type="hidden" name="company_id" value="{company_id}">
             <input type="hidden" name="channel_id" value="{channel_id}">
             <label class="block font-semibold">Channel override</label>
             <select name="policy_override" class="rounded-lg bg-slate-950 p-2">
               <option value="inherit" {inherit}>Inherit company default</option>
               <option value="autonomous" {channel_autonomous}>Autonomous</option>
               <option value="review_all_external" {channel_review}>Review all external responses</option>
             </select>
             <label class="block text-sm">Preferred reviewer principal ID (blank uses fallback order)</label>
             <input name="preferred_reviewer_principal_id" value="{preferred}" class="w-full rounded-lg bg-slate-950 p-2">
             <button class="rounded-lg bg-indigo-700 px-4 py-2" type="submit">Save channel policy</button>
           </form>"#,
        effective = escape_html_text(policy.effective.as_str()),
        preferred = escape_html_attr(&preferred),
    );
    base_layout("Response review policy", &body)
}

fn selected(value: bool) -> &'static str {
    if value { "selected" } else { "" }
}

pub fn response_review_policy_saved(company_id: uuid::Uuid, channel_id: uuid::Uuid) -> String {
    format!(
        r#"<div class="rounded-xl border border-emerald-700 p-4">Policy saved.
        <a class="ml-2 underline" href="/reviews/policy?company_id={company_id}&amp;channel_id={channel_id}">Review settings</a></div>"#
    )
}
