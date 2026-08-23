//! Shared provider, model, and API-key controls used by model configuration forms.

use super::*;

pub(crate) struct ModelConnectionFields<'a> {
    pub agent_id_suffix: Option<&'a str>,
    pub provider: &'a str,
    pub model: &'a str,
    pub api_key: &'a str,
    pub api_key_placeholder: &'a str,
}

pub(crate) fn model_connection_fields(fields: &ModelConnectionFields<'_>) -> String {
    let field_id = |name: &str| {
        fields
            .agent_id_suffix
            .map(|suffix| {
                format!(
                    r#" id="agent-{}-{}""#,
                    escape_html_text(name),
                    escape_html_text(suffix)
                )
            })
            .unwrap_or_default()
    };
    let selected = |provider: &str| {
        if fields.provider == provider {
            " selected"
        } else {
            ""
        }
    };
    let custom_provider =
        !fields.provider.is_empty() && fields.provider != "google" && fields.provider != "openai";
    let custom_selected = if custom_provider { " selected" } else { "" };
    let custom_hidden = if custom_provider { "" } else { " hidden" };
    let model_option = |model: &str| {
        let selected = if fields.model == model {
            " selected"
        } else {
            ""
        };
        format!(r#"<option value="{model}"{selected}>{model}</option>"#)
    };
    let model_options = match fields.provider {
        "google" => format!(
            "{}{}",
            model_option("gemini-3.6-flash"),
            model_option("gemini-3.7-flash")
        ),
        "openai" => format!(
            "{}{}",
            model_option("gpt-5.6-sol"),
            model_option("gpt-5.6-terra")
        ),
        _ => String::new(),
    };
    let model_select_hidden = if custom_provider { " hidden" } else { "" };
    let model_select_disabled = if fields.provider.is_empty() {
        " disabled"
    } else {
        ""
    };
    let model_prompt = if fields.provider.is_empty() {
        "Select provider first"
    } else {
        "Select model"
    };

    format!(
        r##"<div class="grid grid-cols-1 gap-4 md:grid-cols-3" data-model-connection>
            <label class="form-control w-full">
                <div class="label"><span class="text-xs opacity-70">LLM Provider</span></div>
                <select class="select w-full font-mono text-sm" onchange="const grid = this.closest('[data-model-connection]'); const providerInput = this.nextElementSibling; const modelSelect = grid.querySelector('[data-model-select]'); const modelInput = grid.querySelector('[data-model-input]'); const custom = this.value === '__custom__'; const models = this.value === 'google' ? ['gemini-3.6-flash', 'gemini-3.7-flash'] : this.value === 'openai' ? ['gpt-5.6-sol', 'gpt-5.6-terra'] : []; providerInput.value = custom ? '' : this.value; providerInput.classList.toggle('hidden', !custom); modelSelect.replaceChildren(new Option(this.value ? 'Select model' : 'Select provider first', ''), ...models.map(model => new Option(model, model))); modelSelect.disabled = !this.value || custom; modelSelect.classList.toggle('hidden', custom); modelInput.value = ''; modelInput.classList.toggle('hidden', !custom); if (custom) providerInput.focus();">
                    <option value="">Server default</option>
                    <option value="google"{google_selected}>google</option>
                    <option value="openai"{openai_selected}>openai</option>
                    <option value="__custom__"{custom_selected}>Custom…</option>
                </select>
                <input type="text"{provider_id} name="provider" value="{provider}" placeholder="Custom provider" autocomplete="off" class="input mt-2 w-full font-mono text-sm{custom_hidden}">
            </label>
            <label class="form-control w-full">
                <div class="label"><span class="text-xs opacity-70">LLM Model</span></div>
                <select data-model-select class="select w-full font-mono text-sm{model_select_hidden}" onchange="this.nextElementSibling.value = this.value"{model_select_disabled}>
                    <option value="">{model_prompt}</option>
                    {model_options}
                </select>
                <input data-model-input type="text"{model_id} name="model" value="{model}" placeholder="Custom model" autocomplete="off" class="input w-full font-mono text-sm{custom_hidden}">
            </label>
            <label class="form-control w-full">
                <div class="label"><span class="text-xs opacity-70">LLM API Key</span></div>
                <input type="password"{api_key_id} name="api_key" value="{api_key}" placeholder="{api_key_placeholder}" class="input w-full font-mono text-sm">
            </label>
        </div>"##,
        provider_id = field_id("provider"),
        model_id = field_id("model"),
        api_key_id = field_id("api-key"),
        google_selected = selected("google"),
        openai_selected = selected("openai"),
        custom_selected = custom_selected,
        custom_hidden = custom_hidden,
        model_select_hidden = model_select_hidden,
        model_select_disabled = model_select_disabled,
        model_prompt = model_prompt,
        model_options = model_options,
        provider = escape_html_text(fields.provider),
        model = escape_html_text(fields.model),
        api_key = escape_html_text(fields.api_key),
        api_key_placeholder = escape_html_text(fields.api_key_placeholder),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_choices_follow_the_provider_and_custom_values_remain_editable() {
        let google = model_connection_fields(&ModelConnectionFields {
            agent_id_suffix: None,
            provider: "google",
            model: "gemini-3.7-flash",
            api_key: "",
            api_key_placeholder: "API key",
        });
        assert!(google.contains(r#"value="gemini-3.6-flash""#));
        assert!(google.contains(r#"value="gemini-3.7-flash" selected"#));
        assert!(!google.contains(r#"<option value="gpt-5.6-sol""#));

        let custom = model_connection_fields(&ModelConnectionFields {
            agent_id_suffix: None,
            provider: "local&lt;provider",
            model: "local/model",
            api_key: "",
            api_key_placeholder: "API key",
        });
        assert!(custom.contains(r#"value="__custom__" selected"#));
        assert!(custom.contains(r#"value="local&amp;lt;provider""#));
        assert!(custom.contains(r#"value="local/model""#));
        assert!(custom.contains("data-model-input type=\"text\""));
    }
}
