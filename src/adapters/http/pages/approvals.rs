//! Human-in-the-loop approval pages reached from an emailed link.

use super::*;

pub fn approval_result_page(title: &str, approval: &HumanApproval, message: &str) -> String {
    let status_badge = match approval.status {
        ApprovalStatus::Approved => {
            r#"<span class="px-3 py-1 text-xs font-semibold rounded-full bg-emerald-500/20 text-emerald-300 border border-emerald-500/30">✓ Approved</span>"#
        }
        ApprovalStatus::Rejected => {
            r#"<span class="px-3 py-1 text-xs font-semibold rounded-full bg-rose-500/20 text-rose-300 border border-rose-500/30">✗ Rejected</span>"#
        }
        ApprovalStatus::Expired => {
            r#"<span class="px-3 py-1 text-xs font-semibold rounded-full bg-amber-500/20 text-amber-300 border border-amber-500/30">⏱ Expired</span>"#
        }
        ApprovalStatus::Pending => {
            r#"<span class="px-3 py-1 text-xs font-semibold rounded-full bg-sky-500/20 text-sky-300 border border-sky-500/30">⏳ Pending</span>"#
        }
    };

    let content = format!(
        r##"
        <div class="max-w-xl mx-auto py-8 text-center">
            <div class="mb-6 flex justify-center">{status_badge}</div>
            <h1 class="text-2xl font-extrabold text-white mb-4">{title}</h1>
            <p class="text-slate-300 text-base leading-relaxed mb-6">{message}</p>

            <div class="bg-slate-900/60 rounded-xl p-5 border border-slate-700/50 text-left text-xs font-mono text-slate-300 space-y-2 mb-8">
                <div><span class="text-slate-500">Action Title:</span> <span class="text-indigo-300 font-bold">{action_title}</span></div>
                <div><span class="text-slate-500">Approver:</span> {approver_email}</div>
                <div><span class="text-slate-500">Action Type:</span> {action_type}</div>
                <div><span class="text-slate-500">Summary:</span> {action_summary}</div>
            </div>

            <a href="/companies" class="inline-block px-5 py-2.5 bg-indigo-600 hover:bg-indigo-500 text-white font-medium rounded-lg transition">
                Return to Dashboard
            </a>
        </div>
        "##,
        status_badge = status_badge,
        title = title,
        message = message,
        action_title = approval.action_title,
        approver_email = approval.approver_email,
        action_type = approval.action_type,
        action_summary = approval.action_summary,
    );

    base_layout(title, &content)
}

pub fn approval_details_page(approval: &HumanApproval) -> String {
    let confirm_link = format!("/approvals/{}?action=confirm", approval.token);
    let reject_link = format!("/approvals/{}?action=reject", approval.token);

    let content = format!(
        r##"
        <div class="max-w-xl mx-auto py-6">
            <div class="text-center mb-6">
                <span class="px-3 py-1 text-xs font-semibold rounded-full bg-sky-500/20 text-sky-300 border border-sky-500/30">⏳ Action Approval Required</span>
                <h1 class="text-2xl font-extrabold text-white mt-3">{action_title}</h1>
            </div>

            <p class="text-slate-300 text-sm mb-6 text-center">{action_summary}</p>

            <div class="bg-slate-900/60 rounded-xl p-5 border border-slate-700/50 text-xs font-mono text-slate-300 space-y-2 mb-8">
                <div><span class="text-slate-500">Approver Email:</span> <span class="text-indigo-300">{approver_email}</span></div>
                <div><span class="text-slate-500">Action Type:</span> {action_type}</div>
                <div><span class="text-slate-500">Expires At:</span> {expires_at}</div>
            </div>

            <div class="flex items-center justify-center gap-4">
                <a href="{confirm_link}" class="px-6 py-3 bg-emerald-600 hover:bg-emerald-500 text-white font-semibold rounded-xl transition shadow-lg shadow-emerald-900/20">
                    ✓ Confirm & Execute
                </a>
                <a href="{reject_link}" class="px-6 py-3 bg-rose-600 hover:bg-rose-500 text-white font-semibold rounded-xl transition shadow-lg shadow-rose-900/20">
                    ✗ Reject
                </a>
            </div>
        </div>
        "##,
        action_title = approval.action_title,
        action_summary = approval.action_summary,
        approver_email = approval.approver_email,
        action_type = approval.action_type,
        expires_at = approval.expires_at,
        confirm_link = confirm_link,
        reject_link = reject_link,
    );

    base_layout("Confirm Action", &content)
}

pub fn channel_approvals_fragment(approvals: &[HumanApproval]) -> String {
    if approvals.is_empty() {
        return r#"<div class="p-4 text-center text-xs text-slate-400">No human-in-the-loop approvals recorded for this channel.</div>"#.to_string();
    }

    let rows: String = approvals
        .iter()
        .map(|a| {
            let badge = match a.status {
                ApprovalStatus::Approved => r#"<span class="px-2 py-0.5 text-xs rounded bg-emerald-500/20 text-emerald-300 border border-emerald-500/30">✓ Approved</span>"#,
                ApprovalStatus::Rejected => r#"<span class="px-2 py-0.5 text-xs rounded bg-rose-500/20 text-rose-300 border border-rose-500/30">✗ Rejected</span>"#,
                ApprovalStatus::Expired => r#"<span class="px-2 py-0.5 text-xs rounded bg-amber-500/20 text-amber-300 border border-amber-500/30">⏱ Expired</span>"#,
                ApprovalStatus::Pending => r#"<span class="px-2 py-0.5 text-xs rounded bg-sky-500/20 text-sky-300 border border-sky-500/30">⏳ Pending</span>"#,
            };

            format!(
                r##"
                <div class="p-3 bg-slate-900/50 rounded-lg border border-slate-800 text-xs flex items-center justify-between">
                    <div>
                        <div class="flex items-center gap-2 mb-1">
                            <span class="font-bold text-white">{action_title}</span>
                            {badge}
                        </div>
                        <p class="text-slate-400">{action_summary} • <span class="text-slate-300">{approver}</span></p>
                    </div>
                    <span class="text-slate-500 font-mono text-[10px]">{created_at}</span>
                </div>
                "##,
                action_title = a.action_title,
                badge = badge,
                action_summary = a.action_summary,
                approver = a.approver_email,
                created_at = a.created_at,
            )
        })
        .collect();

    format!(r#"<div class="space-y-2 mt-4">{rows}</div>"#, rows = rows)
}
