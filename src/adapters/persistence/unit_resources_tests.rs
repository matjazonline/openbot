//! Plan07 B1: skills and connections belong to a unit, and its manager configures them.
use super::*;
use crate::application::workflow::http::{HttpConnectionWrite, HttpConnections, HttpOrigin};
use crate::entities::mcp::{CompanyMcpConnection, McpAuth, McpEndpoint};
use crate::services::mcp_client::{McpClient, McpSession};
use crate::use_cases::company::CompanyPersistence;
use crate::use_cases::mcp::{McpConnectionWrite, McpUseCases, UnavailableMcpDiscovery};
use crate::use_cases::skill::{SkillUseCases, SkillWrite};
use secrecy::SecretString;
use std::sync::Arc;

struct NoNetwork;
#[async_trait::async_trait]
impl McpClient for NoNetwork {
    async fn connect_prepared(
        &self,
        _: &McpEndpoint,
        _: SecretString,
        _: crate::application::services::mcp_client::McpPreparation,
    ) -> crate::application::services::mcp_client::McpPreparationResult<Box<dyn McpSession>> {
        Err(crate::application::services::mcp_client::McpPreparationFailure::Unavailable)
    }
    async fn shutdown(&self) -> AppResult<()> {
        Ok(())
    }
    fn validate_endpoint(&self, _: &McpEndpoint) -> AppResult<()> {
        Ok(())
    }
    async fn connect(
        &self,
        _: &McpEndpoint,
        _: Option<SecretString>,
    ) -> AppResult<Box<dyn McpSession>> {
        panic!("unit configuration never connects to a provider")
    }
}

fn skills(d: &Delegated) -> SkillUseCases {
    let p = Arc::new(d.f.persistence.clone());
    SkillUseCases::new(p.clone(), p.clone(), p)
}

fn mcp(d: &Delegated) -> McpUseCases {
    let p = Arc::new(d.f.persistence.clone());
    McpUseCases::new(
        p.clone(),
        p.clone(),
        p,
        Arc::new(NoNetwork),
        Arc::new(UnavailableMcpDiscovery),
    )
}

fn skill_write(slug: &str) -> SkillWrite {
    SkillWrite {
        slug: slug.into(),
        name: slug.into(),
        description: "A unit skill".into(),
        instructions: "Follow the unit's policy.".into(),
        files: None,
        required_tools: None,
        created_by: None,
    }
}

fn mcp_write(slug: &str) -> McpConnectionWrite {
    McpConnectionWrite {
        slug: slug.into(),
        endpoint: format!("https://{slug}.example.test/mcp")
            .try_into()
            .unwrap(),
        enabled: true,
        auth: McpAuth::None,
        discovered_tools: vec![],
        tool_grants: vec![],
    }
}

fn actor(user: Uuid) -> WorkflowActor {
    WorkflowActor::authenticated(user).unwrap()
}

async fn http(
    d: &Delegated,
    user: Uuid,
    unit: Option<DepartmentId>,
    host: &str,
) -> AppResult<crate::application::workflow::http::HttpConnection> {
    d.f.persistence
        .create_http(
            d.company(),
            actor(user),
            unit,
            HttpConnectionWrite {
                origin: HttpOrigin::parse(&format!("https://{host}.example.test")).unwrap(),
                bearer: false,
            },
        )
        .await
}

fn refused<T: std::fmt::Debug>(result: AppResult<T>) {
    assert!(matches!(result, Err(AppError::NotFound(_))), "{result:?}");
}

async fn sql_error(pool: &PgPool, sql: &str, binds: &[Uuid]) -> String {
    let mut query = sqlx::query(sql);
    for bind in binds {
        query = query.bind(*bind);
    }
    match query.execute(pool).await {
        Ok(_) => panic!("{sql} was accepted"),
        Err(sqlx::Error::Database(error)) => error.code().unwrap_or_default().into_owned(),
        Err(error) => panic!("{error}"),
    }
}

#[tokio::test]
async fn unit_resource_schema_keys_checks_and_default_unit() {
    let Some(database) = own_database().await else {
        return;
    };
    let d = Delegated::on(database.pool.clone()).await;
    let pool = d.f.pool();
    let company = d.f.company;
    let foreign_root = d.f.persistence.test_root_unit(d.f.foreign_company).await;
    let provenance = r#"'{"actor_type":"system","actor_id":null,"actor_name":"Fixture"}'"#;
    let skill = |unit: &str, owner: &str| {
        format!(
            "INSERT INTO skills(id,company_id,unit_id,slug,name,description,instructions,created_by) \
             VALUES(gen_random_uuid(),{owner},{unit},'shape','Shape','Shape','Shape.',{provenance})"
        )
    };
    // Every company skill has a unit and a library skill has none (a CHECK); the unit is the
    // company's own (the composite key).
    assert_eq!(
        sql_error(pool, &skill("NULL", "$1"), &[company]).await,
        "23514"
    );
    assert_eq!(
        sql_error(pool, &skill("$1", "NULL"), &[d.unit.id.as_uuid()]).await,
        "23514"
    );
    assert_eq!(
        sql_error(pool, &skill("$2", "$1"), &[company, foreign_root]).await,
        "23503"
    );
    // A live MCP connection has a unit; an HTTP connection always has one.
    assert_eq!(
        sql_error(pool, "INSERT INTO company_mcp_connections(company_id,id,slug,endpoint_url,auth_type) VALUES($1,gen_random_uuid(),'x','https://x.test/mcp','none')", &[company]).await,
        "23514"
    );
    assert_eq!(
        sql_error(pool, "INSERT INTO workflow_http_connections(company_id,id,origin,bearer) VALUES($1,gen_random_uuid(),'https://x.test',false)", &[company]).await,
        "23502"
    );
    assert_eq!(
        sql_error(pool, "INSERT INTO workflow_http_connections(company_id,id,origin,bearer,unit_id) VALUES($1,gen_random_uuid(),'https://x.test',false,$2)", &[company, foreign_root]).await,
        "23503"
    );

    // The default unit: the root for a root manager, the one led department, else none.
    let root = d.f.persistence.test_root_unit(company).await;
    let default_pool = pool.clone();
    let default = move |user: Uuid| {
        let pool = default_pool.clone();
        async move {
            sqlx::query_scalar::<_, Option<Uuid>>("SELECT resource_default_unit($1,$2)")
                .bind(user)
                .bind(company)
                .fetch_one(&pool)
                .await
                .unwrap()
        }
    };
    assert_eq!(default(d.f.owner).await, Some(root));
    assert_eq!(default(d.f.admin).await, Some(root));
    assert_eq!(
        default(d.f.member).await,
        Some(d.unit.id.as_uuid()),
        "the lead of one unit"
    );
    assert_eq!(default(d.f.stranger).await, None);
    let (two_unit_lead, person) = teammate(pool, company, "member").await;
    for unit in [d.child.id, d.sibling.id] {
        d.f.use_cases
            .assign_role(d.f.owner, company, unit, DepartmentRole::Lead, person)
            .await
            .unwrap();
    }
    assert_eq!(
        default(two_unit_lead).await,
        None,
        "two led units are ambiguous"
    );
    let (root_lead, root_person) = teammate(pool, company, "member").await;
    d.f.use_cases
        .assign_role(
            d.f.owner,
            company,
            DepartmentId::new(root),
            DepartmentRole::Lead,
            root_person,
        )
        .await
        .unwrap();
    assert_eq!(
        default(root_lead).await,
        Some(root),
        "a root lead manages the root"
    );

    // Creation applies it: a choice is required when there is no default, a stranger gets 404.
    let cases = skills(&d);
    let created = cases
        .create_company(d.f.member, company, None, skill_write("sales-default"))
        .await
        .unwrap();
    assert_eq!(created.unit_id, Some(d.unit.id.as_uuid()));
    let created = cases
        .create_company(d.f.admin, company, None, skill_write("admin-default"))
        .await
        .unwrap();
    assert_eq!(created.unit_id, Some(root));
    assert!(matches!(
        cases.create_company(two_unit_lead, company, None, skill_write("ambiguous")).await,
        Err(AppError::BadRequest(message)) if message == "Choose a unit for this resource."
    ));
    let chosen = cases
        .create_company(
            two_unit_lead,
            company,
            Some(d.sibling.id.as_uuid()),
            skill_write("support-chosen"),
        )
        .await
        .unwrap();
    assert_eq!(chosen.unit_id, Some(d.sibling.id.as_uuid()));
    refused(
        cases
            .create_company(d.f.stranger, company, None, skill_write("stranger"))
            .await,
    );
    refused(
        cases
            .create_company(d.f.member, company, Some(root), skill_write("root-by-lead"))
            .await,
    );
    let made = http(&d, two_unit_lead, None, "ambiguous").await;
    assert!(matches!(made, Err(AppError::BadRequest(_))), "{made:?}");
    assert_eq!(
        http(&d, d.f.member, None, "sales").await.unwrap().unit_id,
        d.unit.id.as_uuid()
    );
    refused(http(&d, d.f.member, Some(d.sibling.id), "sibling").await);
    let connections = mcp(&d);
    assert_eq!(
        connections
            .create(d.f.member, company, None, mcp_write("sales"))
            .await
            .unwrap()
            .unit_id,
        Some(d.unit.id.as_uuid())
    );
    refused(
        connections
            .create(d.f.member, company, Some(root), mcp_write("root-by-lead"))
            .await,
    );
}

async fn unit_of(pool: &PgPool, table: &str, id: Uuid) -> (Option<Uuid>, Option<i64>) {
    let revision = if table == "skills" {
        "NULL::bigint"
    } else {
        "revision"
    };
    sqlx::query_as(&format!(
        "SELECT unit_id,{revision} FROM {table} WHERE id=$1"
    ))
    .bind(id)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn unit_resource_authority_each_write_follows_the_resource_unit() {
    let Some(database) = own_database().await else {
        return;
    };
    let d = Delegated::on(database.pool.clone()).await;
    let company = d.f.company;
    let pool = d.f.pool();
    let (cases, connections) = (skills(&d), mcp(&d));
    let sales = d.unit.id.as_uuid();
    let skill = cases
        .create_company(d.f.owner, company, Some(sales), skill_write("sales-skill"))
        .await
        .unwrap();
    let field_skill = cases
        .create_company(
            d.f.owner,
            company,
            Some(d.child.id.as_uuid()),
            skill_write("field-skill"),
        )
        .await
        .unwrap();
    let server = connections
        .create(d.f.owner, company, Some(sales), mcp_write("sales-server"))
        .await
        .unwrap();
    let endpoint = http(&d, d.f.owner, Some(d.unit.id), "sales-api")
        .await
        .unwrap();
    let before = (
        unit_of(pool, "skills", skill.id).await,
        unit_of(pool, "company_mcp_connections", server.id).await,
        unit_of(pool, "workflow_http_connections", endpoint.id).await,
    );

    // The sibling unit's lead sees none of them and changes nothing.
    let sibling = d.sibling_lead;
    assert!(
        cases
            .get_company(sibling, company, skill.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        cases
            .list_company_page(
                sibling,
                company,
                crate::use_cases::skill::SkillPageRequest {
                    before: None,
                    limit: 50
                }
            )
            .await
            .unwrap()
            .items
            .is_empty()
    );
    refused(
        cases
            .update_company(sibling, company, skill.id, skill_write("taken"))
            .await,
    );
    refused(cases.delete_company(sibling, company, skill.id).await);
    refused(
        cases
            .move_company(sibling, company, skill.id, d.sibling.id.as_uuid())
            .await,
    );
    refused(connections.get(sibling, company, server.id).await);
    assert!(connections.list(sibling, company).await.unwrap().is_empty());
    refused(
        connections
            .update(sibling, &server, mcp_write("sales-server"))
            .await,
    );
    refused(connections.delete(sibling, &server).await);
    refused(connections.replace_token(sibling, &server, None).await);
    refused(
        connections
            .move_to(sibling, company, server.id, d.sibling.id.as_uuid())
            .await,
    );
    let level = |user: Uuid, connection: Uuid, revision: i64| {
        crate::application::workflow::connection_level::ConnectionLevelCommand {
            actor: actor(user),
            company: d.company(),
            connection,
            expected_revision: revision,
            access_level: crate::entities::access_level::AccessLevel::PUBLIC,
        }
    };
    refused(
        connections
            .set_level(level(sibling, server.id, server.revision))
            .await,
    );
    assert!(
        d.f.persistence
            .list_http(d.company(), actor(sibling))
            .await
            .unwrap()
            .is_empty()
    );
    refused(
        d.f.persistence
            .set_http_level(level(sibling, endpoint.id, endpoint.revision))
            .await,
    );
    refused(
        d.f.persistence
            .set_http_enabled(
                d.company(),
                actor(sibling),
                endpoint.id,
                endpoint.revision,
                false,
            )
            .await,
    );
    refused(
        d.f.persistence
            .replace_http_secret(
                d.company(),
                actor(sibling),
                endpoint.id,
                endpoint.revision,
                None,
            )
            .await,
    );
    refused(
        d.f.persistence
            .move_http(d.company(), actor(sibling), endpoint.id, d.sibling.id)
            .await,
    );
    refused(
        d.f.persistence
            .revoke_http_operation(
                d.company(),
                actor(sibling),
                endpoint.id,
                endpoint.revision,
                crate::domain::workflow::TypeName::parse("lookup").unwrap(),
            )
            .await,
    );
    refused(
        crate::application::workflow::mcp::McpOperations::revoke_mcp_operation(
            &d.f.persistence,
            d.company(),
            actor(sibling),
            crate::application::workflow::mcp::McpOperationRevoke {
                connection: server.id,
                catalog_revision: server.revision,
                name: crate::domain::workflow::TypeName::parse("lookup").unwrap(),
                version: Uuid::new_v4(),
            },
        )
        .await,
    );
    // Copying a library skill into a unit needs managing that unit too.
    let library = crate::use_cases::skill::SkillManagementPersistence::create_library(
        &d.f.persistence,
        skill_write("library-guide"),
    )
    .await
    .unwrap();
    refused(
        cases
            .create_skill_from_library(sibling, company, Some(sales), library.id)
            .await,
    );
    let copied = cases
        .create_skill_from_library(d.f.member, company, None, library.id)
        .await
        .unwrap();
    assert_eq!(copied.unit_id, Some(sales), "the lead's default unit");
    assert_eq!(
        before,
        (
            unit_of(pool, "skills", skill.id).await,
            unit_of(pool, "company_mcp_connections", server.id).await,
            unit_of(pool, "workflow_http_connections", endpoint.id).await,
        ),
        "refused writes change nothing"
    );

    // The unit's lead configures them, including a child unit's; a root admin configures any.
    let member = d.f.member;
    assert_eq!(
        cases
            .get_company(member, company, field_skill.id)
            .await
            .unwrap()
            .map(|s| s.id),
        Some(field_skill.id)
    );
    cases
        .update_company(member, company, field_skill.id, skill_write("field-skill"))
        .await
        .unwrap();
    let listed: Vec<Uuid> = connections
        .list(member, company)
        .await
        .unwrap()
        .into_iter()
        .map(|c| c.id)
        .collect();
    assert_eq!(listed, vec![server.id]);
    let updated = connections
        .update(d.f.admin, &server, mcp_write("sales-server"))
        .await
        .unwrap();
    assert_eq!(updated.unit_id, Some(sales));
    let disabled =
        d.f.persistence
            .set_http_enabled(
                d.company(),
                actor(member),
                endpoint.id,
                endpoint.revision,
                false,
            )
            .await
            .unwrap();
    assert!(!disabled.enabled);
    assert_eq!(
        d.f.persistence
            .list_http(d.company(), actor(member))
            .await
            .unwrap()
            .into_iter()
            .map(|c| c.id)
            .collect::<Vec<_>>(),
        vec![endpoint.id]
    );
    cases
        .delete_company(member, company, skill.id)
        .await
        .unwrap();
    // A member who manages nothing does not even reach the lists.
    refused(connections.list(d.f.stranger, company).await);
    refused(
        d.f.persistence
            .list_http(d.company(), actor(d.f.stranger))
            .await,
    );
}

#[tokio::test]
async fn unit_resource_move_needs_both_units_and_changes_only_the_unit() {
    let Some(database) = own_database().await else {
        return;
    };
    let d = Delegated::on(database.pool.clone()).await;
    let company = d.f.company;
    let pool = d.f.pool();
    let (cases, connections) = (skills(&d), mcp(&d));
    let (sales, support) = (d.unit.id, d.sibling.id);
    let server = connections
        .create(
            d.f.owner,
            company,
            Some(sales.as_uuid()),
            mcp_write("moving"),
        )
        .await
        .unwrap();
    let endpoint = http(&d, d.f.owner, Some(sales), "moving").await.unwrap();
    let skill = cases
        .create_company(
            d.f.owner,
            company,
            Some(sales.as_uuid()),
            skill_write("moving"),
        )
        .await
        .unwrap();
    // The Sales lead does not manage Support: no move, nothing changed.
    refused(
        connections
            .move_to(d.f.member, company, server.id, support.as_uuid())
            .await,
    );
    refused(
        d.f.persistence
            .move_http(d.company(), actor(d.f.member), endpoint.id, support)
            .await,
    );
    refused(
        cases
            .move_company(d.f.member, company, skill.id, support.as_uuid())
            .await,
    );
    assert_eq!(
        unit_of(pool, "company_mcp_connections", server.id).await.0,
        Some(sales.as_uuid())
    );
    // Into a child of a unit they manage is allowed for the same lead.
    let moved = cases
        .move_company(d.f.member, company, skill.id, d.child.id.as_uuid())
        .await
        .unwrap();
    assert_eq!((moved.from, moved.to), (sales, d.child.id));
    // The owner manages both: only the unit changes, revisions stay (G1 r2 F6).
    let credential_before: i64 =
        sqlx::query_scalar("SELECT credential_revision FROM company_mcp_connections WHERE id=$1")
            .bind(server.id)
            .fetch_one(pool)
            .await
            .unwrap();
    let moved = connections
        .move_to(d.f.owner, company, server.id, support.as_uuid())
        .await
        .unwrap();
    assert_eq!(
        (moved.from, moved.to, moved.uses.len(), moved.more_uses),
        (sales, support, 0, 0)
    );
    assert_eq!(
        unit_of(pool, "company_mcp_connections", server.id).await,
        (Some(support.as_uuid()), Some(server.revision))
    );
    let credential_after: i64 =
        sqlx::query_scalar("SELECT credential_revision FROM company_mcp_connections WHERE id=$1")
            .bind(server.id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(credential_before, credential_after);
    let moved =
        d.f.persistence
            .move_http(d.company(), actor(d.f.owner), endpoint.id, support)
            .await
            .unwrap();
    assert_eq!((moved.from, moved.to), (sales, support));
    assert_eq!(
        unit_of(pool, "workflow_http_connections", endpoint.id).await,
        (Some(support.as_uuid()), Some(endpoint.revision))
    );
    // A move to where it already is changes nothing.
    let same =
        d.f.persistence
            .move_http(d.company(), actor(d.f.owner), endpoint.id, support)
            .await
            .unwrap();
    assert_eq!((same.from, same.to), (support, support));
    // The Sales lead no longer configures the moved connection: its edit form is now stale.
    let stale: CompanyMcpConnection = server.clone();
    refused(
        connections
            .update(d.f.member, &stale, mcp_write("moving"))
            .await,
    );
    // A move to a unit of another company is the same not-found.
    let foreign_root = DepartmentId::new(d.f.persistence.test_root_unit(d.f.foreign_company).await);
    refused(
        connections
            .move_to(d.f.owner, company, server.id, foreign_root.as_uuid())
            .await,
    );
}

#[tokio::test]
async fn unit_resource_schema_delete_names_what_to_move_and_teardown_keeps_working() {
    let Some(database) = own_database().await else {
        return;
    };
    let d = Delegated::on(database.pool.clone()).await;
    let company = d.f.company;
    let (cases, connections) = (skills(&d), mcp(&d));
    let field = d.child.id;
    cases
        .create_company(
            d.f.owner,
            company,
            Some(field.as_uuid()),
            skill_write("field-guide"),
        )
        .await
        .unwrap();
    let server = connections
        .create(
            d.f.owner,
            company,
            Some(field.as_uuid()),
            mcp_write("field-crm"),
        )
        .await
        .unwrap();
    let endpoint = http(&d, d.f.owner, Some(field), "field-api").await.unwrap();
    // Its own desk goes first (decision 9); then only the unit's resources hold it.
    d.f.drop_desk(field).await;
    let refused_delete =
        d.f.use_cases
            .delete_department(d.f.owner, company, field)
            .await;
    match refused_delete {
        Err(AppError::Conflict(message)) => {
            for name in [
                "skill field-guide",
                "MCP connection field-crm",
                "HTTP connection https://field-api.example.test",
            ] {
                assert!(message.contains(name), "{message}");
            }
            assert!(message.ends_with("Move them first."), "{message}");
        }
        other => panic!("{other:?}"),
    }
    // A deleted MCP connection releases its unit (G1 r2 F5); moved resources release it too.
    let current = connections
        .get(d.f.owner, company, server.id)
        .await
        .unwrap();
    connections.delete(d.f.owner, &current).await.unwrap();
    let deleted: (Option<Uuid>,) =
        sqlx::query_as("SELECT unit_id FROM company_mcp_connections WHERE id=$1")
            .bind(server.id)
            .fetch_one(d.f.pool())
            .await
            .unwrap();
    assert_eq!(deleted.0, None);
    let skill = cases
        .list_company_page(
            d.f.owner,
            company,
            crate::use_cases::skill::SkillPageRequest {
                before: None,
                limit: 50,
            },
        )
        .await
        .unwrap()
        .items
        .into_iter()
        .find(|s| s.slug.as_str() == "field-guide")
        .unwrap();
    cases
        .move_company(d.f.owner, company, skill.id, d.unit.id.as_uuid())
        .await
        .unwrap();
    let still =
        d.f.use_cases
            .delete_department(d.f.owner, company, field)
            .await;
    assert!(
        matches!(&still, Err(AppError::Conflict(message)) if message.contains("HTTP connection") && !message.contains("skill field-guide")),
        "{still:?}"
    );
    d.f.persistence
        .move_http(d.company(), actor(d.f.owner), endpoint.id, d.unit.id)
        .await
        .unwrap();
    d.f.use_cases
        .delete_department(d.f.owner, company, field)
        .await
        .unwrap();

    // Deleting the company with resources still owned by a non-root unit cascades cleanly.
    cases
        .create_company(
            d.f.owner,
            company,
            Some(d.sibling.id.as_uuid()),
            skill_write("support-guide"),
        )
        .await
        .unwrap();
    connections
        .create(
            d.f.owner,
            company,
            Some(d.sibling.id.as_uuid()),
            mcp_write("support-crm"),
        )
        .await
        .unwrap();
    http(&d, d.f.owner, Some(d.sibling.id), "support-api")
        .await
        .unwrap();
    CompanyPersistence::delete_for_user(&d.f.persistence, d.f.owner, company)
        .await
        .unwrap();
    let left: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM skills WHERE company_id=$1) + (SELECT count(*) FROM company_mcp_connections WHERE company_id=$1) + (SELECT count(*) FROM workflow_http_connections WHERE company_id=$1)",
    )
    .bind(company)
    .fetch_one(d.f.pool())
    .await
    .unwrap();
    assert_eq!(left, 0);
}

#[tokio::test]
async fn unit_resource_authority_root_lead_level_ceiling_refresh_and_placed_defaults() {
    let Some(database) = own_database().await else {
        return;
    };
    let d = Delegated::on(database.pool.clone()).await;
    let company = d.f.company;
    let pool = d.f.pool();
    let (cases, connections) = (skills(&d), mcp(&d));
    let root = d.f.persistence.test_root_unit(company).await;
    // A root lead manages the root, so root-owned resources are theirs to configure.
    let (root_lead, root_person) = teammate(pool, company, "member").await;
    d.f.use_cases
        .assign_role(
            d.f.owner,
            company,
            DepartmentId::new(root),
            DepartmentRole::Lead,
            root_person,
        )
        .await
        .unwrap();
    let skill = cases
        .create_company(root_lead, company, None, skill_write("root-guide"))
        .await
        .unwrap();
    assert_eq!(skill.unit_id, Some(root));
    cases
        .update_company(root_lead, company, skill.id, skill_write("root-guide"))
        .await
        .unwrap();
    let server = connections
        .create(root_lead, company, None, mcp_write("root-crm"))
        .await
        .unwrap();
    assert_eq!(server.unit_id, Some(root));
    assert_eq!(
        http(&d, root_lead, None, "root-api").await.unwrap().unit_id,
        root
    );
    refused(
        cases
            .get_company(d.f.member, company, skill.id)
            .await
            .and_then(|found| found.ok_or_else(|| AppError::NotFound("hidden".into()))),
    );

    // A unit lead who manages the connection still cannot raise it above their own level.
    let sales = connections
        .create(
            d.f.owner,
            company,
            Some(d.unit.id.as_uuid()),
            mcp_write("sales-crm"),
        )
        .await
        .unwrap();
    // The Sales lead holds level 2: within it they set the connection's level, above it is 403.
    sqlx::query(
        "UPDATE department_members SET access_level=2 WHERE company_id=$1 AND root_user_id=$2",
    )
    .bind(company)
    .bind(d.f.member)
    .execute(pool)
    .await
    .unwrap();
    let level = |connection: Uuid, revision: i64, value: i16| {
        crate::application::workflow::connection_level::ConnectionLevelCommand {
            actor: actor(d.f.member),
            company: d.company(),
            connection,
            expected_revision: revision,
            access_level: crate::entities::access_level::AccessLevel::try_from(value).unwrap(),
        }
    };
    let within = connections
        .set_level(level(sales.id, sales.revision, 2))
        .await
        .unwrap();
    assert_eq!(within.access_level.as_i16(), 2);
    let above = connections
        .set_level(level(sales.id, within.revision, 3))
        .await;
    assert!(matches!(above, Err(AppError::Forbidden(_))), "{above:?}");
    let endpoint = http(&d, d.f.owner, Some(d.unit.id), "sales-levels")
        .await
        .unwrap();
    let within =
        d.f.persistence
            .set_http_level(level(endpoint.id, endpoint.revision, 2))
            .await
            .unwrap();
    assert_eq!(within.access_level.as_i16(), 2);
    let above =
        d.f.persistence
            .set_http_level(level(endpoint.id, within.revision, 3))
            .await;
    assert!(matches!(above, Err(AppError::Forbidden(_))), "{above:?}");

    // Testing a connection is configuring it: a sibling lead is refused before any connect.
    let owner = crate::application::integrations::VerifiedOwner::from_verified_client(
        crate::adapters::integrations::owner::owner_for_user(d.sibling_lead),
    );
    refused(connections.refresh(d.sibling_lead, &sales, owner).await);

    // An owner or admin placed in a department still defaults to the root they manage.
    for (user, principal) in [(d.f.owner, d.f.owner_principal)] {
        d.f.use_cases
            .add_member(d.f.owner, company, d.unit.id, principal)
            .await
            .unwrap();
        let default: Option<Uuid> = sqlx::query_scalar("SELECT resource_default_unit($1,$2)")
            .bind(user)
            .bind(company)
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(default, Some(root));
    }
    let (admin, admin_principal) = teammate(pool, company, "admin").await;
    d.f.use_cases
        .assign_role(
            d.f.owner,
            company,
            d.unit.id,
            DepartmentRole::Lead,
            admin_principal,
        )
        .await
        .unwrap();
    let default: Option<Uuid> = sqlx::query_scalar("SELECT resource_default_unit($1,$2)")
        .bind(admin)
        .bind(company)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(
        default,
        Some(root),
        "an admin who also leads a unit defaults to the root"
    );
}

/// B1-F1/F2: any unit manager gets an editable Skills tab, and agent authoring still offers the
/// company's skills (Plan06 U3) while the configuration list shows only managed units' skills.
#[tokio::test]
async fn unit_resource_skills_tab_and_agent_authoring_reads() {
    let Some(database) = own_database().await else {
        return;
    };
    let d = Delegated::on(database.pool.clone()).await;
    let company = d.f.company;
    let cases = skills(&d);
    for user in [d.f.owner, d.f.admin, d.f.member, d.sibling_lead] {
        assert!(cases.manages_skills(user, company).await.unwrap());
    }
    assert!(!cases.manages_skills(d.f.stranger, company).await.unwrap());
    let root = cases
        .create_company(d.f.owner, company, None, skill_write("root-policy"))
        .await
        .unwrap();
    let support = cases
        .create_company(
            d.f.owner,
            company,
            Some(d.sibling.id.as_uuid()),
            skill_write("support-policy"),
        )
        .await
        .unwrap();
    let sales = cases
        .create_company(d.f.member, company, None, skill_write("sales-policy"))
        .await
        .unwrap();
    let page = || crate::use_cases::skill::SkillPageRequest {
        before: None,
        limit: 50,
    };
    let ids = |items: Vec<crate::entities::skill::SkillSummary>| {
        let mut ids: Vec<Uuid> = items.into_iter().map(|skill| skill.id).collect();
        ids.sort();
        ids
    };
    // B2 (decision d): a new agent of the Sales lead can be placed in Sales or below, so its
    // offers are what reaches from there: Sales and the root, not the sibling Support.
    let mut reachable = vec![root.id, sales.id];
    reachable.sort();
    assert_eq!(
        ids(cases
            .list_authoring_page(d.f.member, company, None, page())
            .await
            .unwrap()
            .items),
        reachable
    );
    let mut every = vec![root.id, support.id, sales.id];
    every.sort();
    assert_eq!(
        ids(cases
            .list_authoring_page(d.f.owner, company, None, page())
            .await
            .unwrap()
            .items),
        every,
        "the owner can place an agent anywhere"
    );
    assert_eq!(
        ids(cases
            .list_company_page(d.f.member, company, page())
            .await
            .unwrap()
            .items),
        vec![sales.id]
    );
    refused(
        cases
            .list_authoring_page(d.f.stranger, company, None, page())
            .await,
    );
}
