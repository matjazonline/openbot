//! The Resend panel of a company's settings: its credentials, and the webhook URL to register.
//!
//! A section of its own rather than more fields in the company form, because it is written by its
//! own three requests -- save, rotate, disconnect -- and each of them answers with this fragment
//! alone. Nesting a second `<form>` inside the company's would not be valid HTML anyway.
//!
//! No secret is ever rendered back. A stored credential shows as a placeholder saying it is
//! stored, and a blank field means "keep it" -- the same contract the model-provider keys use, and
//! the reason a rejected save can re-render safely.

use super::*;

use crate::entities::company_resend_api::CompanyResendApiIntegration;

/// The `authserv-id` a company gets unless it says otherwise. Resend's own, because Resend is the
/// receiving MTA for every account this panel configures.
pub const DEFAULT_AUTHSERV_ID: &str = "resend.com";

/// What a rejected save puts back in the form.
///
/// Carries no credential by construction: the two secret fields are always rendered blank, so
/// there is nothing here for a re-render to leak.
#[derive(Debug, Clone)]
pub struct CompanyResendApiDraft<'a> {
    pub authserv_id: &'a str,
    pub enabled: bool,
}

/// Everything the panel draws itself from.
pub struct CompanyResendApiSection<'a> {
    pub company_id: Uuid,
    /// `None` when this company has not connected Resend. The row existing is what "connected"
    /// means -- both credentials are stored together or not at all.
    pub integration: Option<&'a CompanyResendApiIntegration>,
    /// The deployment's own origin, e.g. `https://example.com`. The webhook URL is built from it
    /// rather than from the request, so what is copied is what Resend can reach.
    pub base_url: &'a str,
    pub draft: Option<&'a CompanyResendApiDraft<'a>>,
    pub error: Option<&'a str>,
    pub notice: Option<&'a str>,
}

pub fn company_resend_api_section(section: &CompanyResendApiSection<'_>) -> String {
    let company_id = section.company_id;
    let stored_authserv = section
        .integration
        .map(|integration| integration.authserv_id.as_str());
    let authserv_id = section
        .draft
        .map(|draft| draft.authserv_id)
        .or(stored_authserv)
        .unwrap_or(DEFAULT_AUTHSERV_ID);
    let enabled = section
        .draft
        .map(|draft| draft.enabled)
        .or_else(|| section.integration.map(|integration| integration.enabled))
        .unwrap_or(true);
    let connected = section.integration.is_some();

    format!(
        r##"
        <section id="company-resend-api" class="mt-6 rounded-box border border-base-300 bg-base-200 p-5" aria-labelledby="company-resend-api-heading">
            <div class="flex flex-wrap items-center justify-between gap-2">
                <div>
                    <h3 id="company-resend-api-heading" class="font-semibold">Resend</h3>
                    <p class="text-[11px] opacity-60">The provider account this company sends its mail through and receives its mail into. Without one, mail goes out over the deployment relay and no webhook is served.</p>
                </div>
                {status}
            </div>
            {error_html}
            {notice_html}
            {webhook}
            <form hx-put="/ui/companies/{company_id}/resend_api" hx-target="#company-resend-api" hx-swap="outerHTML"
                class="mt-4 space-y-4" aria-busy="false">
                <div class="grid grid-cols-1 gap-4 md:grid-cols-2">
                    <label class="form-control w-full">
                        <div class="label"><span class="text-xs opacity-70">API key</span></div>
                        <input type="password" name="api_key" value="" placeholder="{secret_placeholder}"
                            autocomplete="new-password" class="input w-full font-mono text-sm">
                        <div class="label"><span class="text-[11px] opacity-60">Sends this company's mail, and fetches the mail its webhook announces.</span></div>
                    </label>
                    <label class="form-control w-full">
                        <div class="label"><span class="text-xs opacity-70">Webhook signing secret</span></div>
                        <input type="password" name="signing_secret" value="" placeholder="{secret_placeholder}"
                            autocomplete="new-password" class="input w-full font-mono text-sm">
                        <div class="label"><span class="text-[11px] opacity-60">The <span class="font-mono">whsec_</span> value Resend shows for the endpoint below.</span></div>
                    </label>
                </div>
                <div class="grid grid-cols-1 gap-4 md:grid-cols-2">
                    <label class="form-control w-full">
                        <div class="label"><span class="text-xs opacity-70">authserv-id</span></div>
                        <input type="text" name="authserv_id" required value="{authserv_id}" placeholder="{default_authserv}"
                            class="input w-full font-mono text-sm">
                        <div class="label"><span class="text-[11px] opacity-60">Only this name's Authentication-Results verdicts are believed.</span></div>
                    </label>
                    <label class="label mt-8 cursor-pointer justify-start gap-2">
                        <input type="checkbox" name="enabled" value="true" class="checkbox checkbox-sm"{enabled_checked}>
                        <span>Integration enabled</span>
                    </label>
                </div>
                <div class="flex flex-wrap items-center gap-3 border-t border-base-300 pt-4">
                    <button type="submit" class="btn btn-primary btn-sm">
                        <span class="loading loading-spinner loading-xs hidden [.htmx-request_&]:inline-block"></span>
                        <span class="[.htmx-request_&]:hidden">{save_label}</span>
                        <span class="hidden [.htmx-request_&]:inline">Saving...</span>
                    </button>
                    {rotate_button}
                    {disconnect_button}
                </div>
            </form>
        </section>
        "##,
        status = status_badge(section.integration),
        error_html = form_error_banner(section.error),
        notice_html = notice_banner(section.notice),
        webhook = webhook_block(section),
        secret_placeholder = if connected {
            "Stored - leave blank to keep"
        } else {
            "Required to connect"
        },
        authserv_id = escape_html_attr(authserv_id),
        default_authserv = DEFAULT_AUTHSERV_ID,
        enabled_checked = if enabled { " checked" } else { "" },
        save_label = if connected {
            "Save Resend settings"
        } else {
            "Connect Resend"
        },
        rotate_button = rotate_button(company_id, connected),
        disconnect_button = disconnect_button(company_id, connected),
    )
}

/// Whether this company's mail actually flows through Resend right now, in one word.
///
/// Three states rather than two: a connected integration that has been switched off is not the
/// same as no integration, and reading "Not connected" on a company whose credentials are still
/// stored is how somebody re-enters a key that was never lost.
fn status_badge(integration: Option<&CompanyResendApiIntegration>) -> &'static str {
    match integration {
        Some(integration) if integration.enabled => {
            r#"<span class="badge badge-success badge-sm">Connected</span>"#
        }
        Some(_) => r#"<span class="badge badge-warning badge-sm">Disabled</span>"#,
        None => r#"<span class="badge badge-ghost badge-sm">Not connected</span>"#,
    }
}

/// The endpoint to paste into Resend, and the button that copies it.
///
/// Absent until the integration exists, because the token is minted by the first save: showing a
/// URL before there is a row would be showing one that answers 404.
fn webhook_block(section: &CompanyResendApiSection<'_>) -> String {
    let Some(integration) = section.integration else {
        return String::new();
    };
    format!(
        r##"
            <div class="mt-4 rounded-box border border-base-300 bg-base-100 p-4">
                <div class="flex flex-wrap items-center justify-between gap-2">
                    <span class="text-xs opacity-70">Inbound webhook URL</span>
                    <button type="button" class="btn btn-ghost btn-xs" data-action="copy-text"
                        data-copy-from="company-resend-api-url" data-copied-label="Copied">Copy</button>
                </div>
                <code id="company-resend-api-url" class="mt-2 block break-all font-mono text-xs">{url}</code>
                <p class="mt-2 text-[11px] opacity-60">Register this as an <span class="font-mono">email.received</span> endpoint in Resend. It is unique to this company; its signing secret is what proves a delivery is Resend's.</p>
            </div>
        "##,
        url = escape_html_text(&integration.webhook_url(section.base_url)),
    )
}

/// Present only once there is a token to replace.
fn rotate_button(company_id: Uuid, connected: bool) -> String {
    if !connected {
        return String::new();
    }
    format!(
        r##"<button type="button" class="btn btn-outline btn-sm"
                        hx-post="/ui/companies/{company_id}/resend_api/token"
                        hx-target="#company-resend-api" hx-swap="outerHTML"
                        hx-confirm="Issue a new webhook URL? Deliveries to the old one stop immediately, so update the endpoint in Resend straight after.">Rotate URL</button>"##
    )
}

fn disconnect_button(company_id: Uuid, connected: bool) -> String {
    if !connected {
        return String::new();
    }
    format!(
        r##"<button type="button" class="btn btn-error btn-outline btn-sm ml-auto"
                        hx-delete="/ui/companies/{company_id}/resend_api"
                        hx-target="#company-resend-api" hx-swap="outerHTML"
                        hx-confirm="Disconnect Resend? The stored API key and signing secret are deleted, and this company's mail falls back to the deployment relay.">Disconnect</button>"##
    )
}

/// A one-line confirmation of what just happened, for the writes that change something the form
/// itself does not show -- a rotated URL, a disconnected account.
fn notice_banner(notice: Option<&str>) -> String {
    match notice {
        Some(message) => format!(
            r##"<div class="alert alert-info mt-4 text-sm">{}</div>"##,
            escape_html_text(message)
        ),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::value_objects::{AuthservId, ResendApiWebhookToken};
    use chrono::Utc;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    fn integration(enabled: bool) -> CompanyResendApiIntegration {
        CompanyResendApiIntegration {
            company_id: Uuid::nil(),
            webhook_token: ResendApiWebhookToken::new(TOKEN),
            authserv_id: AuthservId::new("mx.example.com"),
            enabled,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn section<'a>(
        integration: Option<&'a CompanyResendApiIntegration>,
        draft: Option<&'a CompanyResendApiDraft<'a>>,
    ) -> String {
        company_resend_api_section(&CompanyResendApiSection {
            company_id: Uuid::nil(),
            integration,
            base_url: "https://example.com",
            draft,
            error: None,
            notice: None,
        })
    }

    #[test]
    fn an_unconnected_company_is_offered_the_form_but_no_url_to_copy() {
        let html = section(None, None);
        assert!(html.contains("Not connected"));
        assert!(html.contains("Connect Resend"));
        assert!(!html.contains("company-resend-api-url"));
        assert!(!html.contains("Rotate URL"));
        assert!(!html.contains("Disconnect"));
    }

    #[test]
    fn a_connected_company_sees_its_own_webhook_url() {
        let integration = integration(true);
        let html = section(Some(&integration), None);
        assert!(html.contains(&format!(
            "https://example.com/webhooks/email/resend_api/{TOKEN}"
        )));
        assert!(html.contains(r#"data-copy-from="company-resend-api-url""#));
        assert!(html.contains("Connected"));
    }

    #[test]
    fn a_switched_off_integration_reads_as_disabled_rather_than_absent() {
        let integration = integration(false);
        let html = section(Some(&integration), None);
        assert!(html.contains("Disabled"));
        assert!(!html.contains("Not connected"));
        // Its URL stays visible: the endpoint is what an operator re-enables against.
        assert!(html.contains("company-resend-api-url"));
    }

    #[test]
    fn the_stored_authserv_id_opens_the_form_and_a_draft_overrides_it() {
        let integration = integration(true);
        assert!(section(Some(&integration), None).contains(r#"value="mx.example.com""#));
        let draft = CompanyResendApiDraft {
            authserv_id: "typed.example.com",
            enabled: false,
        };
        let html = section(Some(&integration), Some(&draft));
        assert!(html.contains(r#"value="typed.example.com""#));
        assert!(!html.contains(r#"class="checkbox checkbox-sm" checked"#));
    }

    #[test]
    fn no_credential_field_is_ever_rendered_with_a_value() {
        let integration = integration(true);
        let html = section(Some(&integration), None);
        for field in ["api_key", "signing_secret"] {
            let marker = format!(r#"name="{field}" value="""#);
            assert!(html.contains(&marker), "{field} must render blank");
        }
    }
}
