use std::sync::Arc;

use async_trait::async_trait;
use tracing::{info, instrument};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::company::Company,
};

#[async_trait]
pub trait CompanyPersistence: Send + Sync {
    async fn create(
        &self,
        user_id: Uuid,
        name: &str,
        slug: &str,
        api_key: Option<&str>,
        provider: Option<&str>,
        model: Option<&str>,
    ) -> AppResult<Company>;
    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Company>>;
    async fn get_by_slug(&self, slug: &str) -> AppResult<Option<Company>>;
    async fn list_by_user_id(&self, user_id: Uuid) -> AppResult<Vec<Company>>;
    async fn update(
        &self,
        id: Uuid,
        name: &str,
        slug: &str,
        api_key: Option<&str>,
        provider: Option<&str>,
        model: Option<&str>,
    ) -> AppResult<Company>;
    async fn delete(&self, id: Uuid) -> AppResult<()>;
}

#[derive(Clone)]
pub struct CompanyUseCases {
    persistence: Arc<dyn CompanyPersistence>,
}

impl CompanyUseCases {
    pub fn new(persistence: Arc<dyn CompanyPersistence>) -> Self {
        Self { persistence }
    }

    #[instrument(skip(self))]
    pub async fn create_company(
        &self,
        user_id: Uuid,
        name: &str,
        slug: &str,
        api_key: Option<&str>,
        provider: Option<&str>,
        model: Option<&str>,
    ) -> AppResult<Company> {
        let name_trimmed = name.trim();
        let slug_clean = slug.trim().to_lowercase().replace(' ', "-");
        let api_key_clean = api_key.map(|s| s.trim()).filter(|s| !s.is_empty());
        let provider_clean = provider.map(|s| s.trim()).filter(|s| !s.is_empty());
        let model_clean = model.map(|s| s.trim()).filter(|s| !s.is_empty());

        if name_trimmed.is_empty() || slug_clean.is_empty() {
            return Err(AppError::Internal(
                "Company name and slug cannot be empty.".into(),
            ));
        }

        info!("Creating company: {} ({}) for user {}", name_trimmed, slug_clean, user_id);
        self.persistence
            .create(user_id, name_trimmed, &slug_clean, api_key_clean, provider_clean, model_clean)
            .await
    }

    #[instrument(skip(self))]
    pub async fn list_user_companies(&self, user_id: Uuid) -> AppResult<Vec<Company>> {
        self.persistence.list_by_user_id(user_id).await
    }

    #[instrument(skip(self))]
    pub async fn get_company(&self, id: Uuid) -> AppResult<Option<Company>> {
        self.persistence.get_by_id(id).await
    }

    #[instrument(skip(self))]
    pub async fn get_company_by_slug(&self, slug: &str) -> AppResult<Option<Company>> {
        self.persistence.get_by_slug(slug.trim()).await
    }

    #[instrument(skip(self))]
    pub async fn update_company(
        &self,
        id: Uuid,
        name: &str,
        slug: &str,
        api_key: Option<&str>,
        provider: Option<&str>,
        model: Option<&str>,
    ) -> AppResult<Company> {
        let name_trimmed = name.trim();
        let slug_clean = slug.trim().to_lowercase().replace(' ', "-");
        let api_key_clean = api_key.map(|s| s.trim()).filter(|s| !s.is_empty());
        let provider_clean = provider.map(|s| s.trim()).filter(|s| !s.is_empty());
        let model_clean = model.map(|s| s.trim()).filter(|s| !s.is_empty());

        if name_trimmed.is_empty() || slug_clean.is_empty() {
            return Err(AppError::Internal(
                "Company name and slug cannot be empty.".into(),
            ));
        }

        info!("Updating company {}: {} ({})", id, name_trimmed, slug_clean);
        self.persistence
            .update(id, name_trimmed, &slug_clean, api_key_clean, provider_clean, model_clean)
            .await
    }

    #[instrument(skip(self))]
    pub async fn delete_company(&self, id: Uuid) -> AppResult<()> {
        info!("Deleting company {}", id);
        self.persistence.delete(id).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use chrono::Utc;

    use super::*;

    struct MockCompanyPersistence {
        companies: Mutex<Vec<Company>>,
    }

    #[async_trait]
    impl CompanyPersistence for MockCompanyPersistence {
        async fn create(
            &self,
            user_id: Uuid,
            name: &str,
            slug: &str,
            api_key: Option<&str>,
            provider: Option<&str>,
            model: Option<&str>,
        ) -> AppResult<Company> {
            let company = Company {
                id: Uuid::new_v4(),
                user_id,
                name: name.to_string(),
                slug: slug.to_string(),
                api_key: api_key.map(|s| s.to_string()),
                provider: provider.map(|s| s.to_string()),
                model: model.map(|s| s.to_string()),
                created_at: Utc::now().naive_utc(),
            };
            self.companies.lock().unwrap().push(company.clone());
            Ok(company)
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

        async fn list_by_user_id(&self, user_id: Uuid) -> AppResult<Vec<Company>> {
            Ok(self
                .companies
                .lock()
                .unwrap()
                .iter()
                .filter(|c| c.user_id == user_id)
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
        ) -> AppResult<Company> {
            let mut list = self.companies.lock().unwrap();
            let company = list
                .iter_mut()
                .find(|c| c.id == id)
                .ok_or_else(|| AppError::Internal("Not found".into()))?;

            company.name = name.to_string();
            company.slug = slug.to_string();
            company.api_key = api_key.map(|s| s.to_string());
            company.provider = provider.map(|s| s.to_string());
            company.model = model.map(|s| s.to_string());
            Ok(company.clone())
        }

        async fn delete(&self, id: Uuid) -> AppResult<()> {
            self.companies.lock().unwrap().retain(|c| c.id != id);
            Ok(())
        }
    }

    #[tokio::test]
    async fn company_crud_flow_works() {
        let persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(Vec::new()),
        });
        let use_cases = CompanyUseCases::new(persistence);
        let user_id = Uuid::new_v4();

        // Create
        let company = use_cases
            .create_company(user_id, "Acme Corp", "acme-corp", Some("key123"), Some("google"), Some("gemini-2.5-flash"))
            .await
            .unwrap();
        assert_eq!(company.name, "Acme Corp");
        assert_eq!(company.slug, "acme-corp");
        assert_eq!(company.api_key.as_deref(), Some("key123"));
        assert_eq!(company.provider.as_deref(), Some("google"));
        assert_eq!(company.model.as_deref(), Some("gemini-2.5-flash"));

        // List
        let list = use_cases.list_user_companies(user_id).await.unwrap();
        assert_eq!(list.len(), 1);

        // Update
        let updated = use_cases
            .update_company(company.id, "Acme Inc", "acme-inc", None, None, None)
            .await
            .unwrap();
        assert_eq!(updated.name, "Acme Inc");

        // Delete
        use_cases.delete_company(company.id).await.unwrap();
        let list_after = use_cases.list_user_companies(user_id).await.unwrap();
        assert_eq!(list_after.len(), 0);
    }
}
