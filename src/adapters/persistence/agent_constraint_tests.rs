use super::*;
use crate::{
    adapters::persistence::test_support::test_pool,
    application::use_cases::{
        company::{CompanyPersistence, CompanyWrite},
        user::UserPersistence,
    },
};

fn refused(error: sqlx::Error, name: &str) {
    let error = error.as_database_error().expect("database constraint");
    assert_eq!(error.code().as_deref(), Some("23514"));
    assert_eq!(error.constraint(), Some(name));
}

struct RejectedProvider {
    provider: Option<&'static str>,
    model: Option<&'static str>,
    constraint: &'static str,
}
const REJECTED_PROVIDERS: [RejectedProvider; 3] = [
    RejectedProvider {
        provider: Some("openai"),
        model: None,
        constraint: "agents_model_pair_check",
    },
    RejectedProvider {
        provider: None,
        model: Some("model"),
        constraint: "agents_model_pair_check",
    },
    RejectedProvider {
        provider: Some("unsupported"),
        model: Some("model"),
        constraint: "agents_provider_check",
    },
];

#[tokio::test]
async fn schema_constraint_agent_provider_and_model_are_one_pair() {
    let pool = test_pool().await.expect("native provider constraints");
    let p = PostgresPersistence::new(pool);
    let suffix = Uuid::new_v4().simple().to_string();
    let owner = p
        .create_user(&suffix, &format!("{suffix}@example.test"), "hash")
        .await
        .unwrap();
    let company = CompanyPersistence::create(
        &p,
        owner.id,
        CompanyWrite {
            name: "Provider constraints".into(),
            slug: suffix,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let agent = AgentPersistence::create(
        &p,
        company.id,
        AgentWrite {
            name: "Provider pair".into(),
            slug: "pair".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    for case in REJECTED_PROVIDERS {
        let error = sqlx::query("UPDATE agents SET provider=$2,model=$3 WHERE id=$1")
            .bind(agent.id)
            .bind(case.provider)
            .bind(case.model)
            .execute(p.pool())
            .await
            .unwrap_err();
        refused(error, case.constraint);
    }
    for provider in ["google", "openai", "anthropic", "groq"] {
        sqlx::query("UPDATE agents SET provider=$2,model='native-model' WHERE id=$1")
            .bind(agent.id)
            .bind(provider)
            .execute(p.pool())
            .await
            .unwrap();
    }
    sqlx::query("UPDATE agents SET provider=NULL,model=NULL WHERE id=$1")
        .bind(agent.id)
        .execute(p.pool())
        .await
        .unwrap();
    assert_provider_lists_match(p.pool()).await;
}

async fn assert_provider_lists_match(pool: &sqlx::PgPool) {
    // Compare the actual schema vocabularies so changing only one list fails CI.
    let definitions: Vec<String> = sqlx::query_scalar("SELECT pg_get_constraintdef(oid) FROM pg_constraint \
        WHERE conname IN ('agents_provider_check','company_model_connections_provider_check') ORDER BY conname")
        .fetch_all(pool).await.unwrap();
    assert_eq!(definitions.len(), 2);
    let names = |definition: &str| -> std::collections::BTreeSet<String> {
        definition
            .split('\'')
            .enumerate()
            .filter(|(index, _)| index % 2 == 1)
            .map(|(_, part)| part.to_string())
            .collect()
    };
    assert_eq!(names(&definitions[0]), names(&definitions[1]));
    assert_eq!(names(&definitions[0]).len(), 4);
}
