//! Structured tracing, initialized before any fallible startup step (02A.2). Reads only
//! `MEMORY_LOG` (an env-filter, default `info`) and `MEMORY_LOG_FORMAT` (`json` or `text`).
use tracing_subscriber::{EnvFilter, fmt};

/// Returns problems to report once logging works, instead of failing before it exists.
pub fn init() -> Vec<String> {
    let mut problems = Vec::new();
    let filter = match std::env::var("MEMORY_LOG") {
        Ok(raw) => EnvFilter::try_new(&raw).unwrap_or_else(|_| {
            problems.push("MEMORY_LOG is not a valid filter; using info".into());
            EnvFilter::new("info")
        }),
        Err(_) => EnvFilter::new("info"),
    };
    let json = match std::env::var("MEMORY_LOG_FORMAT").as_deref() {
        Ok("text") => false,
        Ok("json") | Err(_) => true,
        Ok(_) => {
            problems.push("MEMORY_LOG_FORMAT must be json or text; using json".into());
            true
        }
    };
    let builder = fmt().with_env_filter(filter).with_writer(std::io::stderr);
    let installed = if json {
        builder.json().try_init()
    } else {
        builder.try_init()
    };
    if installed.is_err() {
        problems.push("a tracing subscriber was already installed".into());
    }
    problems
}
