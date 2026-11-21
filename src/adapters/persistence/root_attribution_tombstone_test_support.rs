//! The imported attribution service owns durable lifecycle evidence after tenant deletion.
use super::PostgresPersistence;
use crate::domain::workflow::CompanyId;
use serde_json::{Value, json};

pub(super) struct Snapshot {
    version: i64,
    events: Vec<Value>,
}

async fn events(p: &PostgresPersistence, company: CompanyId) -> Vec<Value> {
    sqlx::query_scalar("SELECT to_jsonb(event) FROM attribution_lifecycle_outbox AS event WHERE company_id=$1 ORDER BY version")
        .bind(company.as_uuid()).fetch_all(p.pool()).await.unwrap()
}

impl Snapshot {
    pub(super) async fn capture(p: &PostgresPersistence, company: CompanyId) -> Self {
        let version = sqlx::query_scalar(
            "SELECT version FROM attribution_company_lifecycles WHERE company_id=$1",
        )
        .bind(company.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
        Self {
            version,
            events: events(p, company).await,
        }
    }

    pub(super) async fn assert_deleted(self, p: &PostgresPersistence, company: CompanyId) {
        let lifecycle: Value = sqlx::query_scalar("SELECT to_jsonb(lifecycle) FROM attribution_company_lifecycles AS lifecycle WHERE company_id=$1")
            .bind(company.as_uuid()).fetch_one(p.pool()).await.unwrap();
        assert_eq!(lifecycle["state"], "deleted");
        assert_eq!(lifecycle["version"], json!(self.version + 1));
        let retained = events(p, company).await;
        assert_eq!(retained.len(), self.events.len() + 1);
        assert_eq!(
            &retained[..self.events.len()],
            self.events.as_slice(),
            "every prior lifecycle event survives with its exact immutable and delivery fields"
        );
        let terminal = retained.last().unwrap();
        assert_eq!(terminal["company_id"], json!(company.as_uuid()));
        assert_eq!(terminal["state"], "deleted");
        assert_eq!(terminal["version"], lifecycle["version"]);
        assert_eq!(terminal["changed_at"], lifecycle["changed_at"]);
    }
}
