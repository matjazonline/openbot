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
    pub participant_emails: Option<Vec<String>>,
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
            participant_emails: db.participant_emails,
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
        participant_emails: Option<Vec<String>>,
        workflow_config: Option<serde_json::Value>,
    ) -> AppResult<Workflow> {
        let uuid = Uuid::new_v4();

        let db = sqlx::query_as!(
            WorkflowDb,
            r#"INSERT INTO workflows (id, company_id, name, slug, participant_emails, workflow_config)
               VALUES ($1, $2, $3, $4, $5, $6)
               RETURNING id, company_id, name, slug, participant_emails, workflow_config, created_at as "created_at!""#,
            uuid,
            company_id,
            name,
            slug,
            participant_emails.as_deref(),
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
            r#"SELECT id, company_id, name, slug, participant_emails, workflow_config, created_at as "created_at!"
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
            r#"SELECT w.id, w.company_id, w.name, w.slug, w.participant_emails, w.workflow_config, w.created_at as "created_at!"
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
            r#"SELECT id, company_id, name, slug, participant_emails, workflow_config, created_at as "created_at!"
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
        participant_emails: Option<Vec<String>>,
        workflow_config: Option<serde_json::Value>,
    ) -> AppResult<Workflow> {
        let db = sqlx::query_as!(
            WorkflowDb,
            r#"UPDATE workflows
               SET name = $1, slug = $2, participant_emails = $3, workflow_config = $4
               WHERE id = $5
               RETURNING id, company_id, name, slug, participant_emails, workflow_config, created_at as "created_at!""#,
            name,
            slug,
            participant_emails.as_deref(),
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

        let company = CompanyPersistence::create(&persistence, owner.id, "Workflow Corp", "wf-corp")
            .await
            .unwrap();

        // 1. Create Workflow
        let emails = vec!["a@example.com".to_string(), "b@example.com".to_string()];
        let config = json!({ "key": "value" });

        let workflow = WorkflowPersistence::create(&persistence, company.id, "Inbound Email", "inbound-email", Some(emails.clone()), Some(config.clone()))
            .await
            .unwrap();

        assert_eq!(workflow.name, "Inbound Email");
        assert_eq!(workflow.slug, "inbound-email");
        assert_eq!(workflow.participant_emails, Some(emails));
        assert_eq!(workflow.workflow_config, Some(config));

        // 2. Get by ID
        let fetched = WorkflowPersistence::get_by_id(&persistence, workflow.id).await.unwrap().unwrap();
        assert_eq!(fetched.id, workflow.id);

        // 3. List by company ID
        let list = WorkflowPersistence::list_by_company_id(&persistence, company.id).await.unwrap();
        assert_eq!(list.len(), 1);

        // 4. Update
        let updated = WorkflowPersistence::update(&persistence, workflow.id, "Inbound Email V2", "inbound-email-v2", None, None)
            .await
            .unwrap();
        assert_eq!(updated.name, "Inbound Email V2");
        assert_eq!(updated.participant_emails, None);

        // 5. Delete
        WorkflowPersistence::delete(&persistence, workflow.id).await.unwrap();
        let list_after = WorkflowPersistence::list_by_company_id(&persistence, company.id).await.unwrap();
        assert_eq!(list_after.len(), 0);

        // Cleanup
        let _ = CompanyPersistence::delete(&persistence, company.id).await;
    }
}
