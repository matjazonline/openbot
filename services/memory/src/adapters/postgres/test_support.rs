//! Disposable databases for DB-backed tests (D2). Reads only `MEMORY_TEST_DATABASE_URL`, a
//! superuser maintenance URL; a missing variable or server fails the test, never skips it.
//! Each test database is owned by a per-process LOGIN role that runs the migrations, so a
//! migration step that needs a superuser fails here as it would in production.
use super::{MIGRATOR, PgStore};
use sha2::{Digest, Sha256};
use sqlx::{
    ConnectOptions, Connection, PgConnection, PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{process::Command, str::FromStr, sync::OnceLock};
use tokio::sync::OnceCell;

pub const URL_VAR: &str = "MEMORY_TEST_DATABASE_URL";

fn admin_options() -> PgConnectOptions {
    let url = std::env::var(URL_VAR).unwrap_or_else(|_| {
        panic!(
            "{URL_VAR} is not set. DB-backed memory tests need a superuser maintenance URL on a \
             disposable or local server (for example postgres://postgres@localhost:5432/postgres). \
             They create and drop memtest_* databases and roles there and never touch any other \
             database. There is no skip switch: an absent database is a failure, not a pass."
        )
    });
    PgConnectOptions::from_str(&url)
        .unwrap_or_else(|_| panic!("{URL_VAR} is not a valid PostgreSQL URL"))
        .disable_statement_logging()
}

fn host_tag() -> &'static str {
    static TAG: OnceLock<String> = OnceLock::new();
    TAG.get_or_init(|| {
        let host = Command::new("hostname")
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
            .unwrap_or_else(|_| "unknown".into());
        format!("{:x}", Sha256::digest(host.as_bytes()))[..6].to_owned()
    })
}

fn prefix() -> String {
    format!("memtest_{}_{}", host_tag(), std::process::id())
}

fn pid_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .is_ok_and(|status| status.success())
}

async fn admin() -> PgConnection {
    PgConnection::connect_with(&admin_options())
        .await
        .unwrap_or_else(|error| {
            panic!("{URL_VAR} is set but the PostgreSQL server is unreachable: {error}")
        })
}

/// Run statements in order on one superuser connection to `database` (test setup only), e.g.
/// `SET session_replication_role = replica` before removing a row its foreign keys protect.
pub async fn superuser_sql_in(database: &str, statements: &[&str]) {
    let mut db = PgConnection::connect_with(&admin_options().database(database))
        .await
        .expect("connect as the maintenance superuser");
    for sql in statements {
        sqlx::query(sql)
            .execute(&mut db)
            .await
            .unwrap_or_else(|error| panic!("{sql}: {error}"));
    }
}

/// Run one statement as the maintenance superuser (test setup only).
pub async fn admin_sql(sql: &str) {
    let mut db = admin().await;
    sqlx::query(sql)
        .execute(&mut db)
        .await
        .unwrap_or_else(|error| panic!("{sql}: {error}"));
}

struct Owner {
    role: String,
    password: String,
}

static OWNER: OnceCell<Owner> = OnceCell::const_new();

/// Drop databases and roles this host left behind from dead processes, then create this
/// process's owner role.
async fn owner() -> &'static Owner {
    OWNER
        .get_or_init(|| async {
            let mut db = admin().await;
            let tag = format!("memtest_{}_", host_tag());
            let stale: Vec<String> =
                sqlx::query_scalar("SELECT datname FROM pg_database WHERE datname LIKE $1 || '%'")
                    .bind(&tag)
                    .fetch_all(&mut db)
                    .await
                    .expect("list test databases");
            let dead = |name: &str| {
                name.strip_prefix(&tag)
                    .and_then(|rest| rest.split('_').next())
                    .and_then(|pid| pid.parse::<u32>().ok())
                    .is_some_and(|pid| !pid_alive(pid))
            };
            for name in stale.iter().filter(|name| dead(name)) {
                let _ = sqlx::query(&format!("DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE)"))
                    .execute(&mut db)
                    .await;
            }
            let roles: Vec<String> =
                sqlx::query_scalar("SELECT rolname FROM pg_roles WHERE rolname LIKE $1 || '%'")
                    .bind(&tag)
                    .fetch_all(&mut db)
                    .await
                    .expect("list test roles");
            for role in roles.iter().filter(|role| dead(role)) {
                let _ = sqlx::query(&format!("DROP ROLE IF EXISTS \"{role}\""))
                    .execute(&mut db)
                    .await;
            }
            let role = format!("{}_owner", prefix());
            let password = format!("{:x}", uuid::Uuid::new_v4().as_u128());
            sqlx::query(&format!("DROP ROLE IF EXISTS \"{role}\""))
                .execute(&mut db)
                .await
                .expect("drop a previous owner role of this pid");
            sqlx::query(&format!(
                "CREATE ROLE \"{role}\" LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE PASSWORD '{password}'"
            ))
            .execute(&mut db)
            .await
            .unwrap_or_else(|error| {
                panic!("{URL_VAR} must name a role that can create roles: {error}")
            });
            Owner { role, password }
        })
        .await
}

/// A migrated database owned by the test owner role, dropped on drop (also while unwinding).
pub struct TestDb {
    pub name: String,
    pub store: PgStore,
    pub owner_options: PgConnectOptions,
}

impl TestDb {
    pub fn pool(&self) -> &PgPool {
        self.store.pool()
    }
}

/// Create an empty database owned by the test role, with or without pgvector.
pub async fn empty_database(with_vector: bool) -> (String, PgConnectOptions) {
    let owner = owner().await;
    let name = format!("{}_{:08x}", prefix(), uuid::Uuid::new_v4().as_u128() as u32);
    let mut db = admin().await;
    sqlx::query(&format!(
        "CREATE DATABASE \"{name}\" OWNER \"{}\"",
        owner.role
    ))
    .execute(&mut db)
    .await
    .unwrap_or_else(|error| panic!("{URL_VAR} must name a role with CREATEDB: {error}"));
    sqlx::query(&format!(
        "REVOKE CONNECT ON DATABASE \"{name}\" FROM PUBLIC"
    ))
    .execute(&mut db)
    .await
    .expect("revoke public connect");
    sqlx::query(&format!(
        "GRANT CONNECT ON DATABASE \"{name}\" TO \"{}\"",
        owner.role
    ))
    .execute(&mut db)
    .await
    .expect("grant owner connect");
    if with_vector {
        let mut fresh = PgConnection::connect_with(&admin_options().database(&name))
            .await
            .expect("connect to the new test database as the superuser");
        sqlx::query("CREATE EXTENSION vector")
            .execute(&mut fresh)
            .await
            .unwrap_or_else(|error| {
                panic!("pgvector must be installed on the test server and creatable: {error}")
            });
    }
    let options = admin_options()
        .database(&name)
        .username(&owner.role)
        .password(&owner.password);
    (name, options)
}

pub async fn test_db() -> TestDb {
    let (name, owner_options) = empty_database(true).await;
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect_with(owner_options.clone())
        .await
        .expect("connect as the test owner role");
    MIGRATOR
        .run(&pool)
        .await
        .expect("the init migration applies as a non-superuser owner");
    TestDb {
        name,
        store: PgStore::new(pool),
        owner_options,
    }
}

pub fn drop_database(name: &str) {
    let name = name.to_owned();
    // Drop on a dedicated thread with its own runtime, so it also runs during unwinding and
    // from inside a test runtime.
    let _ = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("drop runtime");
        runtime.block_on(async move {
            let mut db = admin().await;
            let _ = sqlx::query(&format!("DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE)"))
                .execute(&mut db)
                .await;
        });
    })
    .join();
}

impl Drop for TestDb {
    fn drop(&mut self) {
        drop_database(&self.name);
    }
}
