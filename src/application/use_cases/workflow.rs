use std::sync::Arc;

use async_trait::async_trait;
use tracing::{info, instrument};
use uuid::Uuid;

use serde::{Deserialize, Serialize};
use crate::{
    app_error::{AppError, AppResult},
    entities::{company::Company, workflow::Workflow},
    use_cases::company::CompanyPersistence,
};

#[async_trait]
pub trait WorkflowPersistence: Send + Sync {
    async fn create(
        &self,
        company_id: Uuid,
        name: &str,
        slug: &str,
        participant_emails: Option<Vec<String>>,
        workflow_config: Option<serde_json::Value>,
    ) -> AppResult<Workflow>;

    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Workflow>>;

    async fn get_by_company_slug_and_workflow_slug(
        &self,
        company_slug: &str,
        workflow_slug: &str,
    ) -> AppResult<Option<Workflow>>;

    async fn list_by_company_id(&self, company_id: Uuid) -> AppResult<Vec<Workflow>>;

    async fn update(
        &self,
        id: Uuid,
        name: &str,
        slug: &str,
        participant_emails: Option<Vec<String>>,
        workflow_config: Option<serde_json::Value>,
    ) -> AppResult<Workflow>;

    async fn delete(&self, id: Uuid) -> AppResult<()>;
}

#[derive(Clone)]
pub struct WorkflowUseCases {
    company_persistence: Arc<dyn CompanyPersistence>,
    workflow_persistence: Arc<dyn WorkflowPersistence>,
}

impl WorkflowUseCases {
    pub fn new(
        company_persistence: Arc<dyn CompanyPersistence>,
        workflow_persistence: Arc<dyn WorkflowPersistence>,
    ) -> Self {
        Self {
            company_persistence,
            workflow_persistence,
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
                "Unauthorized: only the company owner can manage workflows.".into(),
            ));
        }

        Ok(())
    }

    #[instrument(skip(self))]
    pub async fn create_workflow(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        name: &str,
        slug: &str,
        participant_emails: Option<Vec<String>>,
        workflow_config: Option<serde_json::Value>,
    ) -> AppResult<Workflow> {
        self.verify_company_owner(user_id, company_id).await?;

        let name_trimmed = name.trim();
        let slug_clean = slug.trim().to_lowercase().replace(' ', "-");

        if name_trimmed.is_empty() || slug_clean.is_empty() {
            return Err(AppError::Internal(
                "Workflow name and slug cannot be empty.".into(),
            ));
        }

        let cleaned_emails = participant_emails.map(|emails| {
            emails
                .into_iter()
                .map(|e| e.trim().to_lowercase())
                .filter(|e| !e.is_empty() && e.contains('@'))
                .collect::<Vec<_>>()
        });

        info!(
            "Creating workflow '{}' ({}) for company {}",
            name_trimmed, slug_clean, company_id
        );

        self.workflow_persistence
            .create(
                company_id,
                name_trimmed,
                &slug_clean,
                cleaned_emails,
                workflow_config,
            )
            .await
    }

    #[instrument(skip(self))]
    pub async fn list_company_workflows(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Vec<Workflow>> {
        self.verify_company_owner(user_id, company_id).await?;
        self.workflow_persistence.list_by_company_id(company_id).await
    }

    #[instrument(skip(self))]
    pub async fn get_company_workflow(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        workflow_id: Uuid,
    ) -> AppResult<Option<Workflow>> {
        self.verify_company_owner(user_id, company_id).await?;
        let workflow = self.workflow_persistence.get_by_id(workflow_id).await?;
        if let Some(ref wf) = workflow {
            if wf.company_id != company_id {
                return Ok(None);
            }
        }
        Ok(workflow)
    }

    #[instrument(skip(self))]
    pub async fn update_workflow(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        workflow_id: Uuid,
        name: &str,
        slug: &str,
        participant_emails: Option<Vec<String>>,
        workflow_config: Option<serde_json::Value>,
    ) -> AppResult<Workflow> {
        self.verify_company_owner(user_id, company_id).await?;

        let workflow = self
            .workflow_persistence
            .get_by_id(workflow_id)
            .await?
            .ok_or_else(|| AppError::Internal("Workflow not found.".into()))?;

        if workflow.company_id != company_id {
            return Err(AppError::Internal(
                "Workflow does not belong to this company.".into(),
            ));
        }

        let name_trimmed = name.trim();
        let slug_clean = slug.trim().to_lowercase().replace(' ', "-");

        if name_trimmed.is_empty() || slug_clean.is_empty() {
            return Err(AppError::Internal(
                "Workflow name and slug cannot be empty.".into(),
            ));
        }

        let cleaned_emails = participant_emails.map(|emails| {
            emails
                .into_iter()
                .map(|e| e.trim().to_lowercase())
                .filter(|e| !e.is_empty() && e.contains('@'))
                .collect::<Vec<_>>()
        });

        info!(
            "Updating workflow {} for company {}: {} ({})",
            workflow_id, company_id, name_trimmed, slug_clean
        );

        self.workflow_persistence
            .update(
                workflow_id,
                name_trimmed,
                &slug_clean,
                cleaned_emails,
                workflow_config,
            )
            .await
    }

    #[instrument(skip(self))]
    pub async fn delete_workflow(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        workflow_id: Uuid,
    ) -> AppResult<()> {
        self.verify_company_owner(user_id, company_id).await?;

        let workflow = self
            .workflow_persistence
            .get_by_id(workflow_id)
            .await?
            .ok_or_else(|| AppError::Internal("Workflow not found.".into()))?;

        if workflow.company_id != company_id {
            return Err(AppError::Internal(
                "Workflow does not belong to this company.".into(),
            ));
        }

        info!(
            "Deleting workflow {} for company {}",
            workflow_id, company_id
        );
        self.workflow_persistence.delete(workflow_id).await
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
            Some((company_slug, workflow_slug)) => {
                info!(
                    "Parsed recipient -> company_slug: '{}', workflow_slug: '{}'",
                    company_slug, workflow_slug
                );

                let company = self.company_persistence.get_by_slug(&company_slug).await?;
                let workflow = self
                    .workflow_persistence
                    .get_by_company_slug_and_workflow_slug(&company_slug, &workflow_slug)
                    .await?;

                let sender_email = extract_email_address(&email.from);

                let sender_authorized =
                    match &workflow.as_ref().and_then(|w| w.participant_emails.as_ref()) {
                        Some(allowed_emails) if !allowed_emails.is_empty() => {
                            allowed_emails.iter().any(|e| e.eq_ignore_ascii_case(&sender_email))
                        }
                        _ => true,
                    };

                let resolved = company.is_some() && workflow.is_some() && sender_authorized;

                if resolved {
                    info!(
                        "Successfully resolved workflow '{}' for company '{}' with authorized sender '{}'",
                        workflow_slug, company_slug, sender_email
                    );
                } else if !sender_authorized {
                    info!(
                        "Unauthorized sender '{}' for workflow '{}@{}' (not in participant_emails)",
                        sender_email, workflow_slug, company_slug
                    );
                } else {
                    info!(
                        "Workflow or company not found for '{}@{}'",
                        workflow_slug, company_slug
                    );
                }

                Ok(InboundEmailResult {
                    resolved,
                    sender_authorized,
                    company_slug: Some(company_slug),
                    workflow_slug: Some(workflow_slug),
                    company,
                    workflow,
                    email,
                })
            }
            None => {
                info!("Could not parse recipient email address: '{}'", email.to);
                Ok(InboundEmailResult {
                    resolved: false,
                    sender_authorized: false,
                    company_slug: None,
                    workflow_slug: None,
                    company: None,
                    workflow: None,
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
    pub workflow_slug: Option<String>,
    pub company: Option<Company>,
    pub workflow: Option<Workflow>,
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

pub fn parse_recipient_address(to_str: &str, app_domain_name: &str) -> Option<(String, String)> {
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

    let workflow_slug = parts[0].trim();
    let domain_part = parts[1].trim();

    if workflow_slug.is_empty() || domain_part.is_empty() {
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

    Some((company_slug.to_string(), workflow_slug.to_string()))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use chrono::Utc;
    use serde_json::json;
    use crate::entities::company::Company;
    use super::*;

    struct MockCompanyPersistence {
        companies: Mutex<Vec<Company>>,
    }

    #[async_trait]
    impl CompanyPersistence for MockCompanyPersistence {
        async fn create(&self, _user_id: Uuid, _name: &str, _slug: &str) -> AppResult<Company> {
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

        async fn update(&self, _id: Uuid, _name: &str, _slug: &str) -> AppResult<Company> {
            unimplemented!()
        }

        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            unimplemented!()
        }
    }

    struct MockWorkflowPersistence {
        workflows: Mutex<Vec<Workflow>>,
    }

    #[async_trait]
    impl WorkflowPersistence for MockWorkflowPersistence {
        async fn create(
            &self,
            company_id: Uuid,
            name: &str,
            slug: &str,
            participant_emails: Option<Vec<String>>,
            workflow_config: Option<serde_json::Value>,
        ) -> AppResult<Workflow> {
            let workflow = Workflow {
                id: Uuid::new_v4(),
                company_id,
                name: name.to_string(),
                slug: slug.to_string(),
                participant_emails,
                workflow_config,
                created_at: Utc::now().naive_utc(),
            };
            self.workflows.lock().unwrap().push(workflow.clone());
            Ok(workflow)
        }

        async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Workflow>> {
            Ok(self
                .workflows
                .lock()
                .unwrap()
                .iter()
                .find(|w| w.id == id)
                .cloned())
        }

        async fn get_by_company_slug_and_workflow_slug(
            &self,
            _company_slug: &str,
            workflow_slug: &str,
        ) -> AppResult<Option<Workflow>> {
            Ok(self
                .workflows
                .lock()
                .unwrap()
                .iter()
                .find(|w| w.slug.eq_ignore_ascii_case(workflow_slug))
                .cloned())
        }

        async fn list_by_company_id(&self, company_id: Uuid) -> AppResult<Vec<Workflow>> {
            Ok(self
                .workflows
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
            participant_emails: Option<Vec<String>>,
            workflow_config: Option<serde_json::Value>,
        ) -> AppResult<Workflow> {
            let mut list = self.workflows.lock().unwrap();
            let workflow = list
                .iter_mut()
                .find(|w| w.id == id)
                .ok_or_else(|| AppError::Internal("Not found".into()))?;

            workflow.name = name.to_string();
            workflow.slug = slug.to_string();
            workflow.participant_emails = participant_emails;
            workflow.workflow_config = workflow_config;
            Ok(workflow.clone())
        }

        async fn delete(&self, id: Uuid) -> AppResult<()> {
            self.workflows.lock().unwrap().retain(|w| w.id != id);
            Ok(())
        }
    }

    #[tokio::test]
    async fn company_owner_workflow_crud_flow_works() {
        let owner_id = Uuid::new_v4();
        let company_id = Uuid::new_v4();

        let company_persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(vec![Company {
                id: company_id,
                user_id: owner_id,
                name: "Acme Corp".to_string(),
                slug: "acme".to_string(),
                created_at: Utc::now().naive_utc(),
            }]),
        });

        let workflow_persistence = Arc::new(MockWorkflowPersistence {
            workflows: Mutex::new(Vec::new()),
        });

        let use_cases = WorkflowUseCases::new(company_persistence, workflow_persistence);

        // 1. Owner creates workflow with participant emails and config
        let emails = vec!["agent1@example.com".to_string(), "agent2@example.com".to_string()];
        let config = json!({ "trigger": "email_received", "action": "forward" });

        let workflow = use_cases
            .create_workflow(
                owner_id,
                company_id,
                "Support Flow",
                "support-flow",
                Some(emails.clone()),
                Some(config.clone()),
            )
            .await
            .unwrap();

        assert_eq!(workflow.name, "Support Flow");
        assert_eq!(workflow.slug, "support-flow");
        assert_eq!(workflow.participant_emails, Some(emails));
        assert_eq!(workflow.workflow_config, Some(config));

        // 2. Non-owner cannot create workflow
        let non_owner_id = Uuid::new_v4();
        let err = use_cases
            .create_workflow(
                non_owner_id,
                company_id,
                "Hacker Flow",
                "hacker-flow",
                None,
                None,
            )
            .await;
        assert!(err.is_err());

        // 3. List workflows for company
        let list = use_cases
            .list_company_workflows(owner_id, company_id)
            .await
            .unwrap();
        assert_eq!(list.len(), 1);

        // 4. Update workflow
        let updated_config = json!({ "trigger": "webhook", "action": "notify" });
        let updated = use_cases
            .update_workflow(
                owner_id,
                company_id,
                workflow.id,
                "Updated Flow",
                "updated-flow",
                None,
                Some(updated_config.clone()),
            )
            .await
            .unwrap();
        assert_eq!(updated.name, "Updated Flow");
        assert_eq!(updated.slug, "updated-flow");
        assert_eq!(updated.participant_emails, None);
        assert_eq!(updated.workflow_config, Some(updated_config));

        // 5. Delete workflow
        use_cases
            .delete_workflow(owner_id, company_id, workflow.id)
            .await
            .unwrap();

        let list_after = use_cases
            .list_company_workflows(owner_id, company_id)
            .await
            .unwrap();
        assert_eq!(list_after.len(), 0);
    }

    #[test]
    fn parse_recipient_address_works() {
        let app_domain = "mailagents.com";

        // Plain email
        let (company_slug, workflow_slug) =
            parse_recipient_address("support@acme.mailagents.com", app_domain).unwrap();
        assert_eq!(company_slug, "acme");
        assert_eq!(workflow_slug, "support");

        // Named email format
        let (company_slug, workflow_slug) = parse_recipient_address(
            "Inbound Handler <inbound-email@wf-corp.mailagents.com>",
            app_domain,
        )
        .unwrap();
        assert_eq!(company_slug, "wf-corp");
        assert_eq!(workflow_slug, "inbound-email");

        // Localhost app domain
        let (company_slug, workflow_slug) =
            parse_recipient_address("trigger@my-company.localhost", "localhost").unwrap();
        assert_eq!(company_slug, "my-company");
        assert_eq!(workflow_slug, "trigger");

        // Invalid formats
        assert!(parse_recipient_address("invalid-email", app_domain).is_none());
        assert!(parse_recipient_address("support@mailagents.com", app_domain).is_none());
    }

    #[tokio::test]
    async fn process_inbound_email_resolves_company_and_workflow() {
        let owner_id = Uuid::new_v4();
        let company_id = Uuid::new_v4();

        let company_persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(vec![Company {
                id: company_id,
                user_id: owner_id,
                name: "Acme Corp".to_string(),
                slug: "acme".to_string(),
                created_at: Utc::now().naive_utc(),
            }]),
        });

        let workflow_persistence = Arc::new(MockWorkflowPersistence {
            workflows: Mutex::new(Vec::new()),
        });

        let use_cases = WorkflowUseCases::new(company_persistence, workflow_persistence);

        let _ = use_cases
            .create_workflow(owner_id, company_id, "Support Flow", "support", None, None)
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
        assert_eq!(result.workflow_slug.as_deref(), Some("support"));
        assert_eq!(result.company.unwrap().name, "Acme Corp");
        assert_eq!(result.workflow.unwrap().name, "Support Flow");

        // Workflow with participant_emails restriction
        let _ = use_cases
            .create_workflow(
                owner_id,
                company_id,
                "Restricted Flow",
                "restricted",
                Some(vec!["agent@example.com".to_string()]),
                None,
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
}
