//! `memory_boundary_` probes run inside the poisoned-parent build (scripts/boundary-check.sh).

#[test]
fn memory_boundary_environment_probe() {
    // The service's own .cargo/config.toml wins over any parent's RUST_MIN_STACK.
    assert_eq!(std::env::var("RUST_MIN_STACK").as_deref(), Ok("2097152"));
    // Parent `.env` files are never loaded: the poison values must be absent.
    for key in ["DATABASE_URL", "SQLX_OFFLINE", "MEMORY_DATABASE_URL"] {
        let value = std::env::var(key).unwrap_or_default();
        assert!(
            !value.contains("memory-poison"),
            "{key} leaked from a parent .env"
        );
    }
    // Parent rustflags would add this cfg; the boundary check refuses such parents.
    let poisoned = cfg!(memory_poison);
    assert!(!poisoned, "a parent build.rustflags leaked into the build");
}

#[test]
fn memory_boundary_sources_do_not_reach_outside_the_service() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut stack = vec![root.join("src")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            for line in text
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
            {
                for forbidden in ["open_bots", "include_str!(\"../../..", "#[path = \"../../"] {
                    assert!(
                        !line.contains(forbidden) || path.ends_with("boundary_tests.rs"),
                        "{} reaches outside the service: {line}",
                        path.display()
                    );
                }
            }
        }
    }
}
