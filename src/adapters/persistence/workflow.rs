use async_trait::async_trait;
use chrono::NaiveDateTime;
use serde::Serialize;
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::workflow::Workflow,
    use_cases::workflow::WorkflowPersistence,
};

#[derive(sqlx::FromRow, Debug, Serialize)]
pub struct WorkflowDb {
    pub id: Uuid,
    pub company_id: Uuid,
    pub name: String,
    pub slug: String,
    pub api_key: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub participant_emails: Option<Vec<String>>,
    pub agent_ids: Option<Vec<Uuid>>,
    pub workflow_config: Option<serde_json::Value>,
    pub created_at: NaiveDateTime,
}

impl From<WorkflowDb> for Workflow {
    fn from(db: WorkflowDb) -> Self {
        Workflow {
            id: db.id,
            company_id: db.company_id,
            name: db.name,
            slug: db.slug,
            api_key: db.api_key,
            provider: db.provider,
            model: db.model,
            participant_emails: db.participant_emails,
            agent_ids: db.agent_ids,
            workflow_config: db.workflow_config,
            created_at: db.created_at,
        }
    }
}

#[async_trait]
impl WorkflowPersistence for PostgresPersistence {
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
        workflow_config: Option<serde_json::Value>,
    ) -> AppResult<Workflow> {
        let uuid = Uuid::new_v4();

        let db = sqlx::query_as!(
            WorkflowDb,
            r#"INSERT INTO workflows (id, company_id, name, slug, api_key, provider, model, participant_emails, agent_ids, workflow_config)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
               RETURNING id, company_id, name, slug, api_key, provider, model, participant_emails, agent_ids, workflow_config, created_at as "created_at!""#,
            uuid,
            company_id,
            name,
            slug,
            api_key,
            provider,
            model,
            participant_emails.as_deref(),
            agent_ids.as_deref(),
            workflow_config
        )
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.into())
    }

    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Workflow>> {
        let db = sqlx::query_as!(
            WorkflowDb,
            r#"SELECT id, company_id, name, slug, api_key, provider, model, participant_emails, agent_ids, workflow_config, created_at as "created_at!"
               FROM workflows WHERE id = $1"#,
            id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.map(Into::into))
    }

    async fn get_by_company_slug_and_workflow_slug(
        &self,
        company_slug: &str,
        workflow_slug: &str,
    ) -> AppResult<Option<Workflow>> {
        let db = sqlx::query_as!(
            WorkflowDb,
            r#"SELECT w.id, w.company_id, w.name, w.slug, w.api_key, w.provider, w.model, w.participant_emails, w.agent_ids, w.workflow_config, w.created_at as "created_at!"
               FROM workflows w
               JOIN companies c ON c.id = w.company_id
               WHERE LOWER(c.slug) = LOWER($1) AND LOWER(w.slug) = LOWER($2)"#,
            company_slug,
            workflow_slug
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.map(Into::into))
    }

    async fn list_by_company_id(&self, company_id: Uuid) -> AppResult<Vec<Workflow>> {
        let db_list = sqlx::query_as!(
            WorkflowDb,
            r#"SELECT id, company_id, name, slug, api_key, provider, model, participant_emails, agent_ids, workflow_config, created_at as "created_at!"
               FROM workflows WHERE company_id = $1 ORDER BY created_at DESC"#,
            company_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db_list.into_iter().map(Into::into).collect())
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
        workflow_config: Option<serde_json::Value>,
    ) -> AppResult<Workflow> {
        let db = sqlx::query_as!(
            WorkflowDb,
            r#"UPDATE workflows
               SET name = $1, slug = $2, api_key = $3, provider = $4, model = $5, participant_emails = $6, agent_ids = $7, workflow_config = $8
               WHERE id = $9
               RETURNING id, company_id, name, slug, api_key, provider, model, participant_emails, agent_ids, workflow_config, created_at as "created_at!""#,
            name,
            slug,
            api_key,
            provider,
            model,
            participant_emails.as_deref(),
            agent_ids.as_deref(),
            workflow_config,
            id
        )
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.into())
    }

    async fn delete(&self, id: Uuid) -> AppResult<()> {
        sqlx::query!("DELETE FROM workflows WHERE id = $1", id)
            .execute(&self.pool)
            .await
            .map_err(AppError::from)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use crate::use_cases::company::CompanyPersistence;
    use crate::use_cases::user::UserPersistence;

    #[tokio::test]
    async fn postgres_workflow_persistence_works() {
        let database_url = match std::env::var("DATABASE_URL") {
            Ok(url) => url,
            Err(_) => return, // Skip test if DATABASE_URL is not set
        };

        let pool = match sqlx::PgPool::connect(&database_url).await {
            Ok(p) => p,
            Err(_) => return,
        };

        let persistence = PostgresPersistence::new(pool);

        // Create owner & company
        let owner_username = format!("owner_{}", Uuid::new_v4().simple());
        let owner_email = format!("{}@example.com", owner_username);
        let _ = persistence.create_user(&owner_username, &owner_email, "hash").await;
        let owner = persistence.get_by_email(&owner_email).await.unwrap().unwrap();

        let company = CompanyPersistence::create(&persistence, owner.id, "Workflow Corp", "wf-corp", None, None, None)
            .await
            .unwrap();

        // 1. Create Workflow
        let emails = vec!["a@example.com".to_string(), "b@example.com".to_string()];
        let config = json!({ "key": "value" });

        let agent_id1 = Uuid::new_v4();
        let agent_id2 = Uuid::new_v4();
        let agent_ids = vec![agent_id1, agent_id2];

        let workflow = WorkflowPersistence::create(
            &persistence,
            company.id,
            "Inbound Email",
            "inbound-email",
            Some("wf_key_123"),
            Some("openai"),
            Some("gpt-4o"),
            Some(emails.clone()),
            Some(agent_ids.clone()),
            Some(config.clone()),
        )
        .await
        .unwrap();

        assert_eq!(workflow.name, "Inbound Email");
        assert_eq!(workflow.slug, "inbound-email");
        assert_eq!(workflow.api_key.as_deref(), Some("wf_key_123"));
        assert_eq!(workflow.provider.as_deref(), Some("openai"));
        assert_eq!(workflow.model.as_deref(), Some("gpt-4o"));
        assert_eq!(workflow.participant_emails, Some(emails));
        assert_eq!(workflow.agent_ids, Some(agent_ids));
        assert_eq!(workflow.workflow_config, Some(config));

        // 2. Get by ID
        let fetched = WorkflowPersistence::get_by_id(&persistence, workflow.id).await.unwrap().unwrap();
        assert_eq!(fetched.id, workflow.id);

        // 3. List by company ID
        let list = WorkflowPersistence::list_by_company_id(&persistence, company.id).await.unwrap();
        assert_eq!(list.len(), 1);

        // 4. Update
        let updated = WorkflowPersistence::update(
            &persistence,
            workflow.id,
            "Inbound Email V2",
            "inbound-email-v2",
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(updated.name, "Inbound Email V2");
        assert_eq!(updated.api_key, None);
        assert_eq!(updated.participant_emails, None);

        // 5. Delete
        WorkflowPersistence::delete(&persistence, workflow.id).await.unwrap();
        let list_after = WorkflowPersistence::list_by_company_id(&persistence, company.id).await.unwrap();
        assert_eq!(list_after.len(), 0);

        // Cleanup
        let _ = CompanyPersistence::delete(&persistence, company.id).await;
    }
}
