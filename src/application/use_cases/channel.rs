use std::sync::Arc;

use async_trait::async_trait;
use tracing::{info, instrument};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{channel::Channel, company::Company},
    infra::config::AppConfig,
    use_cases::company::CompanyPersistence,
};
use serde::{Deserialize, Serialize};

#[async_trait]
pub trait ChannelPersistence: Send + Sync {
    async fn create(
        &self,
        company_id: Uuid,
        name: &str,
        slug: &str,
        api_key: Option<&str>,
        provider: Option<&str>,
        model: Option<&str>,
        participant_emails: Option<Vec<String>>,
        agent_ids: Option<Vec<Uuid>>,
        channel_config: Option<serde_json::Value>,
    ) -> AppResult<Channel>;

    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Channel>>;

    async fn get_by_company_slug_and_channel_slug(
        &self,
        company_slug: &str,
        channel_slug: &str,
    ) -> AppResult<Option<Channel>>;

    async fn list_by_company_id(&self, company_id: Uuid) -> AppResult<Vec<Channel>>;

    async fn update(
        &self,
        id: Uuid,
        name: &str,
        slug: &str,
        api_key: Option<&str>,
        provider: Option<&str>,
        model: Option<&str>,
        participant_emails: Option<Vec<String>>,
        agent_ids: Option<Vec<Uuid>>,
        channel_config: Option<serde_json::Value>,
    ) -> AppResult<Channel>;

    async fn delete(&self, id: Uuid) -> AppResult<()>;
}

#[derive(Clone)]
pub struct ChannelUseCases {
    company_persistence: Arc<dyn CompanyPersistence>,
    channel_persistence: Arc<dyn ChannelPersistence>,
    config: Arc<AppConfig>,
}

impl ChannelUseCases {
    pub fn new(
        company_persistence: Arc<dyn CompanyPersistence>,
        channel_persistence: Arc<dyn ChannelPersistence>,
        config: Arc<AppConfig>,
    ) -> Self {
        Self {
            company_persistence,
            channel_persistence,
            config,
        }
    }

    async fn verify_company_owner(&self, user_id: Uuid, company_id: Uuid) -> AppResult<()> {
        let company = self
            .company_persistence
            .get_by_id(company_id)
            .await?
            .ok_or_else(|| AppError::Internal("Company not found.".into()))?;

        if company.user_id != user_id {
            return Err(AppError::Internal(
                "Unauthorized: only the company owner can manage channels.".into(),
            ));
        }

        Ok(())
    }

    #[instrument(skip(self))]
    pub async fn create_channel(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        name: &str,
        slug: &str,
        api_key: Option<&str>,
        provider: Option<&str>,
        model: Option<&str>,
        participant_emails: Option<Vec<String>>,
        agent_ids: Option<Vec<Uuid>>,
        channel_config: Option<serde_json::Value>,
        confirm_spam_disabled: bool,
    ) -> AppResult<Channel> {
        self.verify_company_owner(user_id, company_id).await?;

        let name_trimmed = name.trim();
        let slug_clean = slug.trim().to_lowercase().replace(' ', "-");

        if name_trimmed.is_empty() || slug_clean.is_empty() {
            return Err(AppError::Internal(
                "Channel name and slug cannot be empty.".into(),
            ));
        }

        let api_key_clean = api_key.map(|s| s.trim()).filter(|s| !s.is_empty());
        let provider_clean = provider.map(|s| s.trim()).filter(|s| !s.is_empty());
        let model_clean = model.map(|s| s.trim()).filter(|s| !s.is_empty());

        let cleaned_emails = participant_emails.map(|emails| {
            emails
                .into_iter()
                .map(|e| e.trim().to_lowercase())
                .filter(|e| !e.is_empty() && e.contains('@'))
                .collect::<Vec<_>>()
        });

        let is_public = cleaned_emails
            .as_ref()
            .map(|emails| emails.iter().any(|e| e == "@public"))
            .unwrap_or(false);

        if is_public && !self.config.is_spam_scan_enabled() && !confirm_spam_disabled {
            return Err(AppError::Internal(
                "Spam scanning is disabled in server configuration. Saving a public channel (@public) requires explicit confirmation (confirm_spam_disabled) that you are aware spam scanning is disabled.".into(),
            ));
        }

        info!(
            "Creating channel '{}' ({}) for company {}",
            name_trimmed, slug_clean, company_id
        );

        self.channel_persistence
            .create(
                company_id,
                name_trimmed,
                &slug_clean,
                api_key_clean,
                provider_clean,
                model_clean,
                cleaned_emails,
                agent_ids,
                channel_config,
            )
            .await
    }

    #[instrument(skip(self))]
    pub async fn list_company_channels(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Vec<Channel>> {
        self.verify_company_owner(user_id, company_id).await?;
        self.channel_persistence
            .list_by_company_id(company_id)
            .await
    }

    #[instrument(skip(self))]
    pub async fn get_company_channel(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        channel_id: Uuid,
    ) -> AppResult<Option<Channel>> {
        self.verify_company_owner(user_id, company_id).await?;
        let channel = self.channel_persistence.get_by_id(channel_id).await?;
        if let Some(ref ch) = channel {
            if ch.company_id != company_id {
                return Ok(None);
            }
        }
        Ok(channel)
    }

    pub fn channel_persistence(&self) -> &Arc<dyn ChannelPersistence> {
        &self.channel_persistence
    }

    #[instrument(skip(self))]
    pub async fn update_channel(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        channel_id: Uuid,
        name: &str,
        slug: &str,
        api_key: Option<&str>,
        provider: Option<&str>,
        model: Option<&str>,
        participant_emails: Option<Vec<String>>,
        agent_ids: Option<Vec<Uuid>>,
        channel_config: Option<serde_json::Value>,
        confirm_spam_disabled: bool,
    ) -> AppResult<Channel> {
        self.verify_company_owner(user_id, company_id).await?;

        let channel = self
            .channel_persistence
            .get_by_id(channel_id)
            .await?
            .ok_or_else(|| AppError::Internal("Channel not found.".into()))?;

        if channel.company_id != company_id {
            return Err(AppError::Internal(
                "Channel does not belong to this company.".into(),
            ));
        }

        let name_trimmed = name.trim();
        let slug_clean = slug.trim().to_lowercase().replace(' ', "-");

        if name_trimmed.is_empty() || slug_clean.is_empty() {
            return Err(AppError::Internal(
                "Channel name and slug cannot be empty.".into(),
            ));
        }

        let api_key_clean = api_key.map(|s| s.trim()).filter(|s| !s.is_empty());
        let provider_clean = provider.map(|s| s.trim()).filter(|s| !s.is_empty());
        let model_clean = model.map(|s| s.trim()).filter(|s| !s.is_empty());

        let cleaned_emails = participant_emails.map(|emails| {
            emails
                .into_iter()
                .map(|e| e.trim().to_lowercase())
                .filter(|e| !e.is_empty() && e.contains('@'))
                .collect::<Vec<_>>()
        });

        let is_public = cleaned_emails
            .as_ref()
            .map(|emails| emails.iter().any(|e| e == "@public"))
            .unwrap_or(false);

        if is_public && !self.config.is_spam_scan_enabled() && !confirm_spam_disabled {
            return Err(AppError::Internal(
                "Spam scanning is disabled in server configuration. Saving a public channel (@public) requires explicit confirmation (confirm_spam_disabled) that you are aware spam scanning is disabled.".into(),
            ));
        }

        info!(
            "Updating channel {} for company {}: {} ({})",
            channel_id, company_id, name_trimmed, slug_clean
        );

        self.channel_persistence
            .update(
                channel_id,
                name_trimmed,
                &slug_clean,
                api_key_clean,
                provider_clean,
                model_clean,
                cleaned_emails,
                agent_ids,
                channel_config,
            )
            .await
    }

    #[instrument(skip(self))]
    pub async fn delete_channel(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        channel_id: Uuid,
    ) -> AppResult<()> {
        self.verify_company_owner(user_id, company_id).await?;

        let channel = self
            .channel_persistence
            .get_by_id(channel_id)
            .await?
            .ok_or_else(|| AppError::Internal("Channel not found.".into()))?;

        if channel.company_id != company_id {
            return Err(AppError::Internal(
                "Channel does not belong to this company.".into(),
            ));
        }

        info!("Deleting channel {} for company {}", channel_id, company_id);
        self.channel_persistence.delete(channel_id).await
    }

    #[instrument(skip(self, email))]
    pub async fn process_inbound_email(
        &self,
        provider: &str,
        email: InboundEmail,
        app_domain_name: &str,
    ) -> AppResult<InboundEmailResult> {
        info!(
            "Processing inbound email via provider '{}' for recipient: {}",
            provider, email.to
        );

        let parsed = parse_recipient_address(&email.to, app_domain_name);
        match parsed {
            Some((company_slug, channel_slug)) => {
                info!(
                    "Parsed recipient -> company_slug: '{}', channel_slug: '{}'",
                    company_slug, channel_slug
                );

                let company = self.company_persistence.get_by_slug(&company_slug).await?;
                let channel = self
                    .channel_persistence
                    .get_by_company_slug_and_channel_slug(&company_slug, &channel_slug)
                    .await?;

                let sender_email = extract_email_address(&email.from);

                let sender_authorized =
                    if let (Some(comp), Some(ch)) = (company.as_ref(), channel.as_ref()) {
                        match &ch.participant_emails {
                            Some(allowed_emails) if !allowed_emails.is_empty() => {
                                let is_public = allowed_emails
                                    .iter()
                                    .any(|e| e.trim().eq_ignore_ascii_case("@public"));
                                let explicitly_listed = allowed_emails.iter().any(|e| {
                                    !e.trim().eq_ignore_ascii_case("@public")
                                        && e.eq_ignore_ascii_case(&sender_email)
                                });
                                is_public || explicitly_listed
                            }
                            _ => self
                                .company_persistence
                                .is_company_team_member(comp.id, &sender_email)
                                .await
                                .unwrap_or(false),
                        }
                    } else {
                        false
                    };

                let resolved = company.is_some() && channel.is_some() && sender_authorized;

                if resolved {
                    info!(
                        "Successfully resolved channel '{}' for company '{}' with authorized sender '{}'",
                        channel_slug, company_slug, sender_email
                    );
                } else if !sender_authorized {
                    info!(
                        "Unauthorized sender '{}' for channel '{}@{}' (not in participant_emails)",
                        sender_email, channel_slug, company_slug
                    );
                } else {
                    info!(
                        "Channel or company not found for '{}@{}'",
                        channel_slug, company_slug
                    );
                }

                Ok(InboundEmailResult {
                    resolved,
                    sender_authorized,
                    company_slug: Some(company_slug),
                    channel_slug: Some(channel_slug),
                    company,
                    channel,
                    email,
                })
            }
            None => {
                info!("Could not parse recipient email address: '{}'", email.to);
                Ok(InboundEmailResult {
                    resolved: false,
                    sender_authorized: false,
                    company_slug: None,
                    channel_slug: None,
                    company: None,
                    channel: None,
                    email,
                })
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundEmail {
    pub to: String,
    pub from: String,
    pub subject: Option<String>,
    pub text_body: Option<String>,
    pub html_body: Option<String>,
    pub raw_payload: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundEmailResult {
    pub resolved: bool,
    pub sender_authorized: bool,
    pub company_slug: Option<String>,
    pub channel_slug: Option<String>,
    pub company: Option<Company>,
    pub channel: Option<Channel>,
    pub email: InboundEmail,
}

pub fn extract_email_address(input: &str) -> String {
    let email_addr = if let (Some(start), Some(end)) = (input.find('<'), input.rfind('>')) {
        if start < end {
            &input[start + 1..end]
        } else {
            input
        }
    } else {
        input
    };
    email_addr.trim().to_lowercase()
}

pub fn parse_recipient_address_pipeline(
    to_str: &str,
    app_domain_name: &str,
) -> Option<(String, Vec<String>)> {
    let email_addr = if let (Some(start), Some(end)) = (to_str.find('<'), to_str.rfind('>')) {
        if start < end {
            &to_str[start + 1..end]
        } else {
            to_str
        }
    } else {
        to_str
    };

    let cleaned = email_addr.trim().to_lowercase();
    let parts: Vec<&str> = cleaned.split('@').collect();
    if parts.len() != 2 {
        return None;
    }

    let channel_part = parts[0].trim();
    let domain_part = parts[1].trim();

    if channel_part.is_empty() || domain_part.is_empty() {
        return None;
    }

    let domain_lower = app_domain_name.trim().to_lowercase();
    let expected_suffix = format!(".{}", domain_lower);

    let company_slug = if domain_part.ends_with(&expected_suffix) {
        &domain_part[..domain_part.len() - expected_suffix.len()]
    } else if domain_part == domain_lower {
        return None;
    } else if let Some(idx) = domain_part.find('.') {
        &domain_part[..idx]
    } else {
        return None;
    };

    if company_slug.is_empty() {
        return None;
    }

    let channel_slugs: Vec<String> = channel_part
        .split('+')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if channel_slugs.is_empty() {
        return None;
    }

    Some((company_slug.to_string(), channel_slugs))
}

pub fn parse_recipient_address(to_str: &str, app_domain_name: &str) -> Option<(String, String)> {
    parse_recipient_address_pipeline(to_str, app_domain_name)
        .map(|(company, channels)| (company, channels[0].clone()))
}

pub fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();

    let mut dp = vec![vec![0; n + 1]; m + 1];

    for i in 0..=m {
        dp[i][0] = i;
    }
    for j in 0..=n {
        dp[0][j] = j;
    }

    for i in 1..=m {
        for j in 1..=n {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }

    dp[m][n]
}

pub fn find_similar_channel_slugs(target: &str, available: &[Channel]) -> Vec<String> {
    let target_clean = target.trim().to_lowercase();
    let mut matches: Vec<(usize, String)> = Vec::new();

    for ch in available {
        let dist = levenshtein_distance(&target_clean, &ch.slug.to_lowercase());
        let max_dist = (ch.slug.len() / 2).max(2);
        if dist <= max_dist && dist > 0 {
            matches.push((dist, ch.slug.clone()));
        }
    }

    matches.sort_by_key(|(d, _)| *d);
    matches.into_iter().map(|(_, slug)| slug).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::company::Company;
    use chrono::Utc;
    use serde_json::json;
    use std::sync::Mutex;

    struct MockCompanyPersistence {
        companies: Mutex<Vec<Company>>,
    }

    #[async_trait]
    impl CompanyPersistence for MockCompanyPersistence {
        async fn create(
            &self,
            _user_id: Uuid,
            _name: &str,
            _slug: &str,
            _api_key: Option<&str>,
            _provider: Option<&str>,
            _model: Option<&str>,
            _enable_llm_spam_guardrail: Option<bool>,
        ) -> AppResult<Company> {
            unimplemented!()
        }

        async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Company>> {
            Ok(self
                .companies
                .lock()
                .unwrap()
                .iter()
                .find(|c| c.id == id)
                .cloned())
        }

        async fn get_by_slug(&self, slug: &str) -> AppResult<Option<Company>> {
            Ok(self
                .companies
                .lock()
                .unwrap()
                .iter()
                .find(|c| c.slug.eq_ignore_ascii_case(slug))
                .cloned())
        }

        async fn list_by_user_id(&self, _user_id: Uuid) -> AppResult<Vec<Company>> {
            unimplemented!()
        }

        async fn update(
            &self,
            _id: Uuid,
            _name: &str,
            _slug: &str,
            _api_key: Option<&str>,
            _provider: Option<&str>,
            _model: Option<&str>,
            _enable_llm_spam_guardrail: Option<bool>,
        ) -> AppResult<Company> {
            unimplemented!()
        }

        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            unimplemented!()
        }

        async fn is_company_team_member(&self, _company_id: Uuid, _email: &str) -> AppResult<bool> {
            Ok(true)
        }

        async fn list_company_team_emails(&self, _company_id: Uuid) -> AppResult<Vec<String>> {
            Ok(vec![])
        }
    }

    struct MockChannelPersistence {
        channels: Mutex<Vec<Channel>>,
    }

    #[async_trait]
    impl ChannelPersistence for MockChannelPersistence {
        async fn create(
            &self,
            company_id: Uuid,
            name: &str,
            slug: &str,
            api_key: Option<&str>,
            provider: Option<&str>,
            model: Option<&str>,
            participant_emails: Option<Vec<String>>,
            agent_ids: Option<Vec<Uuid>>,
            channel_config: Option<serde_json::Value>,
        ) -> AppResult<Channel> {
            let channel = Channel {
                id: Uuid::new_v4(),
                company_id,
                name: name.to_string(),
                slug: slug.to_string(),
                api_key: api_key.map(|s| s.to_string()),
                provider: provider.map(|s| s.to_string()),
                model: model.map(|s| s.to_string()),
                participant_emails,
                agent_ids,
                channel_config,
                created_at: Utc::now().naive_utc(),
            };
            self.channels.lock().unwrap().push(channel.clone());
            Ok(channel)
        }

        async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Channel>> {
            Ok(self
                .channels
                .lock()
                .unwrap()
                .iter()
                .find(|w| w.id == id)
                .cloned())
        }

        async fn get_by_company_slug_and_channel_slug(
            &self,
            _company_slug: &str,
            channel_slug: &str,
        ) -> AppResult<Option<Channel>> {
            Ok(self
                .channels
                .lock()
                .unwrap()
                .iter()
                .find(|w| w.slug.eq_ignore_ascii_case(channel_slug))
                .cloned())
        }

        async fn list_by_company_id(&self, company_id: Uuid) -> AppResult<Vec<Channel>> {
            Ok(self
                .channels
                .lock()
                .unwrap()
                .iter()
                .filter(|w| w.company_id == company_id)
                .cloned()
                .collect())
        }

        async fn update(
            &self,
            id: Uuid,
            name: &str,
            slug: &str,
            api_key: Option<&str>,
            provider: Option<&str>,
            model: Option<&str>,
            participant_emails: Option<Vec<String>>,
            agent_ids: Option<Vec<Uuid>>,
            channel_config: Option<serde_json::Value>,
        ) -> AppResult<Channel> {
            let mut list = self.channels.lock().unwrap();
            let channel = list
                .iter_mut()
                .find(|w| w.id == id)
                .ok_or_else(|| AppError::Internal("Not found".into()))?;

            channel.name = name.to_string();
            channel.slug = slug.to_string();
            channel.api_key = api_key.map(|s| s.to_string());
            channel.provider = provider.map(|s| s.to_string());
            channel.model = model.map(|s| s.to_string());
            channel.participant_emails = participant_emails;
            channel.agent_ids = agent_ids;
            channel.channel_config = channel_config;
            Ok(channel.clone())
        }

        async fn delete(&self, id: Uuid) -> AppResult<()> {
            self.channels.lock().unwrap().retain(|w| w.id != id);
            Ok(())
        }
    }

    fn test_config(spam_enabled: bool) -> Arc<AppConfig> {
        Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            access_token_ttl: time::Duration::days(1),
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
            smtp_host: "localhost".to_string(),
            smtp_port: 1025,
            smtp_username: "".to_string(),
            smtp_password: "".to_string(),
            smtp_from_address: "noreply@mailagents.com".to_string(),
            incoming_smtp_enabled: true,
            incoming_smtp_host: "0.0.0.0".to_string(),
            incoming_smtp_port: 2525,
            max_spam_score: 5.0,
            dnsbl_enabled: false,
            dnsbl_servers: vec![],
            smtp_rate_limit_conns_per_ip: 30,
            reject_self_domain_helo: true,
            enable_heuristic_scanner: spam_enabled,
            enable_spam_scanner: false,
            spam_scanner_type: "rspamd".to_string(),
            spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
            enable_llm_spam_guardrail: false,
        })
    }

    #[tokio::test]
    async fn company_owner_channel_crud_flow_works() {
        let owner_id = Uuid::new_v4();
        let company_id = Uuid::new_v4();

        let company_persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(vec![Company {
                id: company_id,
                user_id: owner_id,
                name: "Acme Corp".to_string(),
                slug: "acme".to_string(),
                api_key: None,
                provider: None,
                model: None,
                enable_llm_spam_guardrail: None,
                created_at: Utc::now().naive_utc(),
            }]),
        });

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(Vec::new()),
        });

        let use_cases =
            ChannelUseCases::new(company_persistence, channel_persistence, test_config(true));

        // 1. Owner creates channel with participant emails and config
        let emails = vec![
            "agent1@example.com".to_string(),
            "agent2@example.com".to_string(),
        ];
        let config = json!({ "trigger": "email_received", "action": "forward" });

        let agent_id1 = Uuid::new_v4();
        let agent_id2 = Uuid::new_v4();
        let agent_ids = vec![agent_id1, agent_id2];

        let channel = use_cases
            .create_channel(
                owner_id,
                company_id,
                "Support Flow",
                "support-flow",
                Some("key_123"),
                Some("openai"),
                Some("gpt-4o"),
                Some(emails.clone()),
                Some(agent_ids.clone()),
                Some(config.clone()),
                false,
            )
            .await
            .unwrap();

        assert_eq!(channel.name, "Support Flow");
        assert_eq!(channel.slug, "support-flow");
        assert_eq!(channel.api_key.as_deref(), Some("key_123"));
        assert_eq!(channel.provider.as_deref(), Some("openai"));
        assert_eq!(channel.model.as_deref(), Some("gpt-4o"));
        assert_eq!(channel.participant_emails, Some(emails));
        assert_eq!(channel.agent_ids, Some(agent_ids));
        assert_eq!(channel.channel_config, Some(config));

        // 2. Non-owner cannot create channel
        let non_owner_id = Uuid::new_v4();
        let err = use_cases
            .create_channel(
                non_owner_id,
                company_id,
                "Hacker Flow",
                "hacker-flow",
                None,
                None,
                None,
                None,
                None,
                None,
                false,
            )
            .await;
        assert!(err.is_err());

        // 3. List channels for company
        let list = use_cases
            .list_company_channels(owner_id, company_id)
            .await
            .unwrap();
        assert_eq!(list.len(), 1);

        // 4. Update channel
        let updated_config = json!({ "trigger": "webhook", "action": "notify" });
        let updated = use_cases
            .update_channel(
                owner_id,
                company_id,
                channel.id,
                "Updated Flow",
                "updated-flow",
                None,
                None,
                None,
                None,
                None,
                Some(updated_config.clone()),
                false,
            )
            .await
            .unwrap();
        assert_eq!(updated.name, "Updated Flow");
        assert_eq!(updated.slug, "updated-flow");
        assert_eq!(updated.participant_emails, None);
        assert_eq!(updated.agent_ids, None);
        assert_eq!(updated.channel_config, Some(updated_config));

        // 5. Delete channel
        use_cases
            .delete_channel(owner_id, company_id, channel.id)
            .await
            .unwrap();

        let list_after = use_cases
            .list_company_channels(owner_id, company_id)
            .await
            .unwrap();
        assert_eq!(list_after.len(), 0);
    }

    #[test]
    fn parse_recipient_address_works() {
        let app_domain = "mailagents.com";

        // Plain email
        let (company_slug, channel_slug) =
            parse_recipient_address("support@acme.mailagents.com", app_domain).unwrap();
        assert_eq!(company_slug, "acme");
        assert_eq!(channel_slug, "support");

        // Named email format
        let (company_slug, channel_slug) = parse_recipient_address(
            "Inbound Handler <inbound-email@wf-corp.mailagents.com>",
            app_domain,
        )
        .unwrap();
        assert_eq!(company_slug, "wf-corp");
        assert_eq!(channel_slug, "inbound-email");

        // Chained pipeline email format
        let (company_slug, channel_slugs) = parse_recipient_address_pipeline(
            "support+billing+legal@acme.mailagents.com",
            app_domain,
        )
        .unwrap();
        assert_eq!(company_slug, "acme");
        assert_eq!(channel_slugs, vec!["support", "billing", "legal"]);

        // Localhost app domain
        let (company_slug, channel_slug) =
            parse_recipient_address("trigger@my-company.localhost", "localhost").unwrap();
        assert_eq!(company_slug, "my-company");
        assert_eq!(channel_slug, "trigger");

        // Invalid formats
        assert!(parse_recipient_address("invalid-email", app_domain).is_none());
        assert!(parse_recipient_address("support@mailagents.com", app_domain).is_none());
    }

    #[test]
    fn test_levenshtein_and_fuzzy_channel_suggestions() {
        assert_eq!(levenshtein_distance("suppport", "support"), 1);
        assert_eq!(levenshtein_distance("biling", "billing"), 1);

        let company_id = uuid::Uuid::new_v4();
        let available = vec![
            Channel {
                id: uuid::Uuid::new_v4(),
                company_id,
                name: "Support".to_string(),
                slug: "support".to_string(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: None,
                agent_ids: None,
                channel_config: None,
                created_at: chrono::Utc::now().naive_utc(),
            },
            Channel {
                id: uuid::Uuid::new_v4(),
                company_id,
                name: "Billing".to_string(),
                slug: "billing".to_string(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: None,
                agent_ids: None,
                channel_config: None,
                created_at: chrono::Utc::now().naive_utc(),
            },
        ];

        let suggestions = find_similar_channel_slugs("suppport", &available);
        assert_eq!(suggestions, vec!["support"]);

        let suggestions_biling = find_similar_channel_slugs("biling", &available);
        assert_eq!(suggestions_biling, vec!["billing"]);
    }

    #[tokio::test]
    async fn process_inbound_email_resolves_company_and_channel() {
        let owner_id = Uuid::new_v4();
        let company_id = Uuid::new_v4();

        let company_persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(vec![Company {
                id: company_id,
                user_id: owner_id,
                name: "Acme Corp".to_string(),
                slug: "acme".to_string(),
                api_key: None,
                provider: None,
                model: None,
                enable_llm_spam_guardrail: None,
                created_at: Utc::now().naive_utc(),
            }]),
        });

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(Vec::new()),
        });

        let use_cases =
            ChannelUseCases::new(company_persistence, channel_persistence, test_config(true));

        let _ = use_cases
            .create_channel(
                owner_id,
                company_id,
                "Support Flow",
                "support",
                None,
                None,
                None,
                None,
                None,
                None,
                false,
            )
            .await
            .unwrap();

        let email = InboundEmail {
            to: "Support <support@acme.mailagents.com>".to_string(),
            from: "customer@example.com".to_string(),
            subject: Some("Need help".to_string()),
            text_body: Some("Hello".to_string()),
            html_body: None,
            raw_payload: None,
        };

        let result = use_cases
            .process_inbound_email("sendgrid", email, "mailagents.com")
            .await
            .unwrap();

        assert!(result.resolved);
        assert!(result.sender_authorized);
        assert_eq!(result.company_slug.as_deref(), Some("acme"));
        assert_eq!(result.channel_slug.as_deref(), Some("support"));
        assert_eq!(result.company.unwrap().name, "Acme Corp");
        assert_eq!(result.channel.unwrap().name, "Support Flow");

        // Channel with participant_emails restriction
        let _ = use_cases
            .create_channel(
                owner_id,
                company_id,
                "Restricted Flow",
                "restricted",
                None,
                None,
                None,
                Some(vec!["agent@example.com".to_string()]),
                None,
                None,
                false,
            )
            .await
            .unwrap();

        // 1. Authorized sender
        let auth_email = InboundEmail {
            to: "restricted@acme.mailagents.com".to_string(),
            from: "Agent Smith <agent@example.com>".to_string(),
            subject: Some("Allowed".to_string()),
            text_body: None,
            html_body: None,
            raw_payload: None,
        };

        let auth_result = use_cases
            .process_inbound_email("sendgrid", auth_email, "mailagents.com")
            .await
            .unwrap();

        assert!(auth_result.resolved);
        assert!(auth_result.sender_authorized);

        // 2. Unauthorized sender
        let unauth_email = InboundEmail {
            to: "restricted@acme.mailagents.com".to_string(),
            from: "stranger@external.com".to_string(),
            subject: Some("Denied".to_string()),
            text_body: None,
            html_body: None,
            raw_payload: None,
        };

        let unauth_result = use_cases
            .process_inbound_email("sendgrid", unauth_email, "mailagents.com")
            .await
            .unwrap();

        assert!(!unauth_result.resolved);
        assert!(!unauth_result.sender_authorized);
    }

    #[tokio::test]
    async fn test_channel_creation_spam_disabled_confirmation() {
        let owner_id = Uuid::new_v4();
        let company_id = Uuid::new_v4();

        let company_persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(vec![Company {
                id: company_id,
                user_id: owner_id,
                name: "Acme Corp".to_string(),
                slug: "acme".to_string(),
                api_key: None,
                provider: None,
                model: None,
                enable_llm_spam_guardrail: None,
                created_at: Utc::now().naive_utc(),
            }]),
        });

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(Vec::new()),
        });

        // Config with spam scanning completely disabled
        let use_cases =
            ChannelUseCases::new(company_persistence, channel_persistence, test_config(false));

        // 1. Trying to create public channel (@public) without confirmation fails when spam scanning is disabled
        let res = use_cases
            .create_channel(
                owner_id,
                company_id,
                "Public Flow",
                "public",
                None,
                None,
                None,
                Some(vec!["@public".to_string()]),
                None,
                None,
                false,
            )
            .await;
        assert!(res.is_err());
        assert!(
            res.unwrap_err()
                .to_string()
                .contains("Spam scanning is disabled")
        );

        // 2. Creating public channel (@public) BUT WITH confirm_spam_disabled=true succeeds
        let res_ok = use_cases
            .create_channel(
                owner_id,
                company_id,
                "Public Flow",
                "public",
                None,
                None,
                None,
                Some(vec!["@public".to_string()]),
                None,
                None,
                true,
            )
            .await;
        assert!(res_ok.is_ok());

        // 3. Creating channel WITH participants even without confirmation succeeds
        let res_participants = use_cases
            .create_channel(
                owner_id,
                company_id,
                "Restricted Flow",
                "restricted",
                None,
                None,
                None,
                Some(vec!["allowed@example.com".to_string()]),
                None,
                None,
                false,
            )
            .await;
        assert!(res_participants.is_ok());
    }
}
