use super::*;

const TOKEN: &str =
    "msk_adm_0190f1a2b3c44d5e8f9a0b1c2d3e4f50_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

fn values(url: &str) -> MemoryServiceValues {
    MemoryServiceValues {
        url: Some(url.into()),
        token: Some(SecretString::from(TOKEN)),
        ..MemoryServiceValues::default()
    }
}

#[test]
fn memory_integration_config_unset_url_means_unavailable_and_refuses_stray_keys() {
    assert!(
        MemoryServiceConfig::from_values(MemoryServiceValues::default())
            .unwrap()
            .is_none()
    );
    let stray = MemoryServiceValues {
        token: Some(SecretString::from(TOKEN)),
        ..MemoryServiceValues::default()
    };
    let error = MemoryServiceConfig::from_values(stray).unwrap_err();
    assert!(error.contains(TOKEN_ENV), "{error}");
}

#[test]
fn memory_integration_config_defaults_and_parsed_credential_id() {
    let config = MemoryServiceConfig::from_values(values("https://memory.example.com/"))
        .unwrap()
        .unwrap();
    assert_eq!(config.base_url, "https://memory.example.com");
    assert_eq!(config.request_timeout, Duration::from_millis(12_000));
    assert_eq!(config.recall_deadline, Duration::from_millis(2_000));
    assert_eq!(
        (config.interactive_in_flight, config.background_in_flight),
        (8, 4)
    );
    assert_eq!(
        config.provisioner.credential_id(),
        Uuid::parse_str("0190f1a2-b3c4-4d5e-8f9a-0b1c2d3e4f50").unwrap()
    );
    // The token never reaches Debug output.
    assert!(!format!("{config:?}").contains("AAAA"));
}

#[test]
fn memory_integration_config_url_rules() {
    for ok in [
        "https://memory.internal",
        "http://127.0.0.1:8085",
        "http://localhost:8085/",
        "http://[::1]:8085",
    ] {
        assert!(MemoryServiceConfig::from_values(values(ok)).is_ok(), "{ok}");
    }
    for bad in [
        "http://memory.internal",
        "ftp://memory.internal",
        "https://user:pass@memory.internal",
        "https://memory.internal/?x=1",
        "https://memory.internal/#f",
        "memory.internal",
    ] {
        assert!(
            MemoryServiceConfig::from_values(values(bad)).is_err(),
            "{bad}"
        );
    }
}

#[test]
fn memory_integration_config_token_and_bounds_are_validated() {
    for token in [
        "",
        "msk_cli_0190f1a2b3c44d5e8f9a0b1c2d3e4f50_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        "msk_adm_0190f1a2_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        "msk_adm_0190f1a2b3c44d5e8f9a0b1c2d3e4f50_short",
    ] {
        let mut input = values("https://memory.internal");
        input.token = Some(SecretString::from(token));
        assert!(MemoryServiceConfig::from_values(input).is_err(), "{token}");
    }
    let mut input = values("https://memory.internal");
    input.token = None;
    assert!(MemoryServiceConfig::from_values(input).is_err());
    for (set, value) in [
        (0, "999"),
        (0, "30001"),
        (1, "249"),
        (1, "12000"),
        (2, "0"),
        (2, "33"),
        (3, "17"),
        (3, "x"),
    ] {
        let mut input = values("https://memory.internal");
        let slot = match set {
            0 => &mut input.timeout,
            1 => &mut input.recall_deadline,
            2 => &mut input.interactive,
            _ => &mut input.background,
        };
        *slot = Some(value.into());
        assert!(
            MemoryServiceConfig::from_values(input).is_err(),
            "{set}:{value}"
        );
    }
}
