//! Explicit configuration from `MEMORY_*` environment variables only (D1). No `.env` file and no
//! other variable is read. Every problem is reported at once, before anything starts.
use crate::adapters::{anthropic::ApiKey, http::HttpSettings, postgres::PoolSettings};
use std::{fmt, net::SocketAddr, path::PathBuf, time::Duration};

/// Stock 2 MiB; equal to `RUST_MIN_STACK` in `.cargo/config.toml`. Raising either requires a
/// stack-budget re-run at 2 MiB in scripts/verify.sh.
pub const DEFAULT_RUNTIME_THREAD_STACK_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone)]
pub struct Config {
    pub bind: SocketAddr,
    pub database_url: String,
    pub pool: PoolSettings,
    pub http: HttpSettings,
    pub shutdown_grace: Duration,
    pub runtime_thread_stack_bytes: usize,
    pub connect_timeout: Duration,
    /// The prepared, manifest-validated model directory (D7); startup fails without it.
    pub model_dir: PathBuf,
    /// Observation generation (04B); `None` without `MEMORY_ANTHROPIC_API_KEY`, and then
    /// generation requests are refused with `generation_disabled` and no worker runs.
    pub generation: Option<GenerationConfig>,
}

/// The Anthropic provider and the generation workers (D9). The model is fixed
/// (`claude-sonnet-5-5`, user decision); startup checks the values' shape only and never
/// contacts the provider.
#[derive(Clone)]
pub struct GenerationConfig {
    pub api_key: ApiKey,
    pub base_url: String,
    pub effort: String,
    pub workers: usize,
}

impl fmt::Debug for GenerationConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GenerationConfig")
            .field("api_key", &"<redacted>")
            .field("base_url", &self.base_url)
            .field("effort", &self.effort)
            .field("workers", &self.workers)
            .finish()
    }
}

/// `https://…`, or `http://` to a loopback host (a local fake provider); no credentials,
/// query or fragment.
fn provider_url(raw: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(raw) else {
        return false;
    };
    let host = url.host_str().unwrap_or_default();
    let loopback = host == "localhost"
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    (url.scheme() == "https" || (url.scheme() == "http" && loopback))
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
}

fn generation(
    get: &impl Fn(&str) -> Option<String>,
    errors: &mut Vec<String>,
) -> Option<GenerationConfig> {
    let workers = number(get, "MEMORY_GENERATION_WORKERS", 4, 1, 4, errors) as usize;
    let base_url = get("MEMORY_ANTHROPIC_BASE_URL");
    let effort = get("MEMORY_ANTHROPIC_EFFORT");
    let Some(key) = get("MEMORY_ANTHROPIC_API_KEY") else {
        if base_url.is_some() || effort.is_some() {
            errors.push(
                "MEMORY_ANTHROPIC_BASE_URL and MEMORY_ANTHROPIC_EFFORT need MEMORY_ANTHROPIC_API_KEY"
                    .into(),
            );
        }
        return None;
    };
    if key.is_empty() || key.len() > 512 || !key.bytes().all(|b| b.is_ascii_graphic()) {
        errors.push("MEMORY_ANTHROPIC_API_KEY must be 1-512 visible ASCII characters".into());
    }
    let base_url = base_url.unwrap_or_else(|| crate::adapters::anthropic::DEFAULT_BASE_URL.into());
    if !provider_url(&base_url) {
        errors.push(
            "MEMORY_ANTHROPIC_BASE_URL must be an https:// URL (http:// only to a loopback host) \
             without credentials or a query"
                .into(),
        );
    }
    let effort = effort.unwrap_or_else(|| "low".into());
    if !["low", "medium", "high"].contains(&effort.as_str()) {
        errors.push("MEMORY_ANTHROPIC_EFFORT must be low, medium or high".into());
    }
    Some(GenerationConfig {
        api_key: ApiKey::new(key),
        base_url,
        effort,
        workers,
    })
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("bind", &self.bind)
            .field("database_url", &"<redacted>")
            .field("pool", &self.pool)
            .field("http", &self.http)
            .field("shutdown_grace", &self.shutdown_grace)
            .field("model_dir", &self.model_dir)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError(pub Vec<String>);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid configuration: {}", self.0.join("; "))
    }
}
impl std::error::Error for ConfigError {}

fn number(
    get: &impl Fn(&str) -> Option<String>,
    key: &str,
    default: u64,
    min: u64,
    max: u64,
    errors: &mut Vec<String>,
) -> u64 {
    match get(key) {
        None => default,
        Some(raw) => match raw.trim().parse::<u64>() {
            Ok(value) if (min..=max).contains(&value) => value,
            _ => {
                errors.push(format!("{key} must be an integer in {min}..={max}"));
                default
            }
        },
    }
}

fn list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let mut errors = Vec::new();
        let bind = match get("MEMORY_BIND_ADDR") {
            None => SocketAddr::from(([127, 0, 0, 1], 8085)),
            Some(raw) => raw.parse().unwrap_or_else(|_| {
                errors.push("MEMORY_BIND_ADDR must be host:port, e.g. 127.0.0.1:8085".into());
                SocketAddr::from(([127, 0, 0, 1], 8085))
            }),
        };
        let database_url = match get("MEMORY_DATABASE_URL") {
            Some(url) if url.starts_with("postgres://") || url.starts_with("postgresql://") => url,
            Some(_) => {
                errors.push("MEMORY_DATABASE_URL must be a postgres:// URL".into());
                String::new()
            }
            None => {
                errors.push("MEMORY_DATABASE_URL is required".into());
                String::new()
            }
        };
        let pool = PoolSettings {
            max_connections: number(&get, "MEMORY_DB_MAX_CONNECTIONS", 16, 1, 64, &mut errors)
                as u32,
            acquire_timeout: Duration::from_millis(number(
                &get,
                "MEMORY_DB_ACQUIRE_TIMEOUT_MS",
                3_000,
                100,
                30_000,
                &mut errors,
            )),
            statement_timeout: Duration::from_millis(number(
                &get,
                "MEMORY_DB_STATEMENT_TIMEOUT_MS",
                5_000,
                100,
                60_000,
                &mut errors,
            )),
        };
        let default_hosts = "localhost,127.0.0.1,::1";
        let hosts_raw = get("MEMORY_MCP_ALLOWED_HOSTS");
        let mcp_allowed_hosts = list(hosts_raw.as_deref().unwrap_or(default_hosts));
        if mcp_allowed_hosts.is_empty() {
            // rmcp treats an empty list as "allow every host"; never pass one.
            errors.push("MEMORY_MCP_ALLOWED_HOSTS must name at least one host".into());
        }
        if !bind.ip().is_loopback() && hosts_raw.is_none() {
            errors.push(
                "MEMORY_MCP_ALLOWED_HOSTS is required when MEMORY_BIND_ADDR is not loopback".into(),
            );
        }
        let http = HttpSettings {
            max_in_flight: number(&get, "MEMORY_MAX_IN_FLIGHT", 64, 1, 64, &mut errors) as usize,
            request_deadline: Duration::from_millis(number(
                &get,
                "MEMORY_REQUEST_DEADLINE_MS",
                10_000,
                100,
                60_000,
                &mut errors,
            )),
            mcp_allowed_hosts,
            mcp_allowed_origins: list(&get("MEMORY_MCP_ALLOWED_ORIGINS").unwrap_or_default()),
        };
        let shutdown_grace = Duration::from_millis(number(
            &get,
            "MEMORY_SHUTDOWN_GRACE_MS",
            10_000,
            0,
            120_000,
            &mut errors,
        ));
        let runtime_thread_stack_bytes = number(
            &get,
            "MEMORY_RUNTIME_THREAD_STACK_BYTES",
            DEFAULT_RUNTIME_THREAD_STACK_BYTES as u64,
            DEFAULT_RUNTIME_THREAD_STACK_BYTES as u64,
            DEFAULT_RUNTIME_THREAD_STACK_BYTES as u64,
            &mut errors,
        ) as usize;
        let connect_timeout = Duration::from_millis(number(
            &get,
            "MEMORY_DB_CONNECT_TIMEOUT_MS",
            10_000,
            100,
            120_000,
            &mut errors,
        ));
        let model_dir = match get("MEMORY_MODEL_DIR") {
            Some(raw) if raw.trim().is_empty() => {
                errors.push("MEMORY_MODEL_DIR must name the prepared model directory".into());
                PathBuf::new()
            }
            Some(raw) => PathBuf::from(raw),
            None => PathBuf::from("assets/model"),
        };
        let generation = generation(&get, &mut errors);
        if errors.is_empty() {
            Ok(Self {
                bind,
                database_url,
                pool,
                http,
                shutdown_grace,
                runtime_thread_stack_bytes,
                connect_timeout,
                model_dir,
                generation,
            })
        } else {
            Err(ConfigError(errors))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn config(pairs: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Config::from_lookup(|key| map.get(key).cloned())
    }

    #[test]
    fn memory_startup_config_requires_a_database_url_and_reports_every_problem() {
        let error = config(&[
            ("MEMORY_DB_MAX_CONNECTIONS", "0"),
            ("MEMORY_BIND_ADDR", "x"),
        ])
        .unwrap_err();
        assert_eq!(error.0.len(), 3, "{error}");
        assert!(
            error
                .to_string()
                .contains("MEMORY_DATABASE_URL is required")
        );
        assert!(error.to_string().contains("MEMORY_DB_MAX_CONNECTIONS"));
        assert!(error.to_string().contains("MEMORY_BIND_ADDR"));
        assert!(config(&[("MEMORY_DATABASE_URL", "mysql://x")]).is_err());
    }

    #[test]
    fn memory_startup_config_defaults_and_bounds() {
        let ok = config(&[("MEMORY_DATABASE_URL", "postgres://u@localhost/m")]).unwrap();
        assert!(ok.bind.ip().is_loopback());
        assert_eq!(ok.pool.max_connections, 16);
        assert_eq!(ok.http.max_in_flight, 64);
        assert_eq!(
            ok.runtime_thread_stack_bytes,
            DEFAULT_RUNTIME_THREAD_STACK_BYTES
        );
        assert!(
            !format!("{ok:?}").contains("postgres://"),
            "the URL is never printed"
        );
        // The raised-limit rule: the stack cannot be raised by configuration alone.
        assert!(
            config(&[
                ("MEMORY_DATABASE_URL", "postgres://u@localhost/m"),
                ("MEMORY_RUNTIME_THREAD_STACK_BYTES", "16777216")
            ])
            .is_err()
        );
        assert!(
            config(&[
                ("MEMORY_DATABASE_URL", "postgres://u@localhost/m"),
                ("MEMORY_MAX_IN_FLIGHT", "65")
            ])
            .is_err()
        );
    }

    #[test]
    fn memory_startup_non_loopback_bind_requires_explicit_mcp_hosts() {
        let url = ("MEMORY_DATABASE_URL", "postgres://u@db/m");
        assert!(config(&[url, ("MEMORY_BIND_ADDR", "0.0.0.0:8085")]).is_err());
        let ok = config(&[
            url,
            ("MEMORY_BIND_ADDR", "0.0.0.0:8085"),
            (
                "MEMORY_MCP_ALLOWED_HOSTS",
                "memory.internal, memory.internal:8085",
            ),
        ])
        .unwrap();
        assert_eq!(
            ok.http.mcp_allowed_hosts,
            vec!["memory.internal", "memory.internal:8085"]
        );
        assert!(config(&[url, ("MEMORY_MCP_ALLOWED_HOSTS", " , ")]).is_err());
    }

    #[test]
    fn memory_startup_generation_config_is_validated_and_the_key_never_printed() {
        let url = ("MEMORY_DATABASE_URL", "postgres://u@db/m");
        // Without a key generation is off; provider settings alone are refused.
        assert!(config(&[url]).unwrap().generation.is_none());
        assert!(config(&[url, ("MEMORY_ANTHROPIC_EFFORT", "low")]).is_err());
        let key = ("MEMORY_ANTHROPIC_API_KEY", "sk-ant-api03-secretvalue");
        let ok = config(&[url, key]).unwrap();
        let generation = ok.generation.as_ref().unwrap();
        assert_eq!(generation.base_url, "https://api.anthropic.com");
        assert_eq!((generation.effort.as_str(), generation.workers), ("low", 4));
        assert!(!format!("{ok:?}").contains("secretvalue"));
        for bad in [
            ("MEMORY_ANTHROPIC_BASE_URL", "http://api.anthropic.com"),
            (
                "MEMORY_ANTHROPIC_BASE_URL",
                "https://user:pw@api.anthropic.com",
            ),
            (
                "MEMORY_ANTHROPIC_BASE_URL",
                "https://api.anthropic.com/?x=1",
            ),
            ("MEMORY_ANTHROPIC_BASE_URL", "not a url"),
            ("MEMORY_ANTHROPIC_EFFORT", "max"),
            ("MEMORY_GENERATION_WORKERS", "5"),
            ("MEMORY_GENERATION_WORKERS", "0"),
        ] {
            assert!(config(&[url, key, bad]).is_err(), "{bad:?}");
        }
        assert!(config(&[url, ("MEMORY_ANTHROPIC_API_KEY", "two words")]).is_err());
        assert!(config(&[url, ("MEMORY_ANTHROPIC_API_KEY", "")]).is_err());
        let local = config(&[
            url,
            key,
            ("MEMORY_ANTHROPIC_BASE_URL", "http://127.0.0.1:9999"),
            ("MEMORY_GENERATION_WORKERS", "2"),
        ])
        .unwrap();
        assert_eq!(local.generation.unwrap().workers, 2);
    }

    #[test]
    fn memory_startup_stack_default_matches_the_libtest_setting() {
        // .cargo/config.toml sets RUST_MIN_STACK for libtest threads; the two move together.
        let configured = include_str!("../../.cargo/config.toml");
        assert!(configured.contains(&format!(
            "RUST_MIN_STACK = \"{DEFAULT_RUNTIME_THREAD_STACK_BYTES}\""
        )));
    }
}
