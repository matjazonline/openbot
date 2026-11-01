//! Model-provider policy shared by validation, orchestration, and provider pickers.

/// Every logical provider a company may configure and an agent may select.
pub const SUPPORTED_MODEL_PROVIDERS: [&str; 5] = ["google", "openai", "anthropic", "groq", "xai"];

/// Whether `provider` is one this deployment knows how to execute.
pub fn is_supported_model_provider(provider: &str) -> bool {
    SUPPORTED_MODEL_PROVIDERS.contains(&provider)
}

/// Curated model choices shown by the shared agent/onboarding form.
///
/// Providers with no curated choices retain the existing custom-model input.
pub fn preset_models(provider: &str) -> &'static [&'static str] {
    match provider {
        "google" => &["gemini-3.6-flash", "gemini-3.7-flash"],
        "openai" => &["gpt-5.6-sol", "gpt-5.6-terra"],
        "anthropic" => &["claude-opus-5-5", "claude-sonnet-5-5"],
        "xai" => &["grok-4.6"],
        _ => &[],
    }
}

/// Model inherited by newly provisioned agents when no explicit model was supplied.
pub fn default_model_for(provider: &str) -> &'static str {
    match provider {
        "openai" => "gpt-4o",
        "anthropic" => "claude-sonnet-5-5",
        "groq" => "llama-3.3-70b-versatile",
        "xai" => "grok-4.6",
        // Unsupported providers are rejected before execution. Keep Google's historical default
        // here for callers that are still resolving an omitted provider.
        _ => "gemini-2.5-flash",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xai_is_supported_with_only_the_grok_4_6_preset() {
        assert!(is_supported_model_provider("xai"));
        assert_eq!(preset_models("xai"), ["grok-4.6"]);
        assert_eq!(default_model_for("xai"), "grok-4.6");
    }

    #[test]
    fn anthropic_offers_the_latest_opus_and_sonnet_and_defaults_to_a_preset() {
        assert!(is_supported_model_provider("anthropic"));
        assert_eq!(
            preset_models("anthropic"),
            ["claude-opus-5-5", "claude-sonnet-5-5"]
        );
        assert!(preset_models("anthropic").contains(&default_model_for("anthropic")));
    }
}
