//! `memory_observation_` pure rules (D9): redaction, the provider-safe schema and the bounded
//! local output validation.
use super::*;

#[test]
fn memory_observation_redaction_strips_private_spans_and_secrets_idempotently() {
    let text = "keep <private>drop me</private> ok <PRIVATE>also\nthis</private> \
                key sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUV and ghp_ABCDEFGHIJKLMNOPQRSTUVWX1234 \
                Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.payload.sig \
                password=hunter2, api_key: \"s3cr3t value\" AKIAABCDEFGHIJKLMNOP \
                msk_cli_0190c7a0-0000-7000-8000-000000000001_secretpart \
                -----BEGIN RSA PRIVATE KEY-----\nMIIEow\n-----END RSA PRIVATE KEY----- tail \
                <private>unclosed to the end";
    let redacted = redact(text);
    for secret in [
        "drop me", "also", "sk-ant", "ghp_", "eyJhbG", "hunter2", "s3cr3t", "AKIA", "msk_cli",
        "MIIEow", "unclosed",
    ] {
        assert!(
            !redacted.text.contains(secret),
            "{secret} in {}",
            redacted.text
        );
    }
    assert!(
        redacted
            .text
            .starts_with("keep  ok  key [REDACTED:api_key]")
    );
    assert!(redacted.text.contains("Bearer [REDACTED:credential]"));
    assert!(redacted.text.contains("password=[REDACTED:secret]"));
    assert!(redacted.text.contains("[REDACTED:private_key] tail"));
    assert_eq!(redacted.private_spans, 3);
    assert_eq!(
        redacted.secrets,
        [
            "private_key",
            "api_key",
            "api_key",
            "api_key",
            "api_key",
            "credential",
            "secret",
            "secret"
        ]
    );
    // Idempotent: the boundary recheck changes nothing more.
    let again = redact(&redacted.text);
    assert_eq!(
        (
            again.text.as_str(),
            again.private_spans,
            again.secrets.len()
        ),
        (redacted.text.as_str(), 0, 0)
    );
    // Ordinary text is untouched.
    let plain = redact("We chose PostgreSQL; the token bucket refills hourly.");
    assert_eq!(
        plain.text,
        "We chose PostgreSQL; the token bucket refills hourly."
    );
}

#[test]
fn memory_observation_redaction_leaves_prose_and_catches_values_around_markers() {
    for prose in [
        "the basic configuration works",
        "Basic authentication requires care",
        "the bearer of bad news",
        "a basic understanding of internationalization",
        "Bearer tokens expire hourly",
        // Review p6a-code-r2 n1: sentence punctuation and hyphenated words are not tokens, and
        // an `Authorization:` value must be credential-shaped.
        "the basic responsibilities.",
        "Basic internationalization.",
        "bearer instrumentation-heavy",
        "Authorization: required for admins",
    ] {
        let redacted = redact(prose);
        assert_eq!(
            (redacted.text.as_str(), redacted.secrets.len()),
            (prose, 0),
            "{prose}"
        );
    }
    for credential in [
        "Basic dXNlcjpwYXNzd29yZA==",
        "bearer abcdefghij0123456789",
        "Authorization: secretvalue1",
        "authorization=Token abcdefgh42",
    ] {
        let redacted = redact(credential);
        assert!(
            redacted.text.ends_with("[REDACTED:credential]"),
            "{credential} -> {}",
            redacted.text
        );
    }
    // A literal marker inside a value does not shield the rest of it.
    let wrapped = redact("token=abc[REDACTED:x]def and password=[REDACTED:secret]");
    assert_eq!(
        wrapped.text,
        "token=[REDACTED:secret] and password=[REDACTED:secret]"
    );
    assert_eq!(wrapped.secrets, ["secret"]);
    // Review p6a-code-r2 m1: keys behind a prefix and URL userinfo passwords.
    for (text, kept) in [
        ("DB_PASSWORD=hunter2", "DB_PASSWORD="),
        ("POSTGRES_PASSWORD: s3cret", "POSTGRES_PASSWORD: "),
        ("client_secret=abc", "client_secret="),
        ("refresh_token=rt-1", "refresh_token="),
        ("id_token=eyJhbGciOi", "id_token="),
        ("GITHUB_TOKEN=plainvalue", "GITHUB_TOKEN="),
        ("MY_API_KEY=mine", "MY_API_KEY="),
        ("export OPENAI_API_KEY=k-123", "export OPENAI_API_KEY="),
        (
            "AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI",
            "AWS_SECRET_ACCESS_KEY=",
        ),
        ("postgres://user:hunter2@db:5432/app", "postgres://user:"),
    ] {
        let redacted = redact(text);
        assert!(
            redacted.text.starts_with(kept)
                && redacted.text.contains("[REDACTED:")
                && ![
                    "hunter2",
                    "s3cret",
                    "abc",
                    "rt-1",
                    "eyJ",
                    "plainvalue",
                    "mine",
                    "k-123",
                    "wJalr"
                ]
                .iter()
                .any(|secret| redacted.text.contains(secret)),
            "{text} -> {}",
            redacted.text
        );
        let again = redact(&redacted.text);
        assert_eq!(
            (again.text, again.secrets.len()),
            (redacted.text, 0),
            "{text}"
        );
    }
    assert_eq!(
        redact("postgres://user:hunter2@db:5432/app").text,
        "postgres://user:[REDACTED:secret]@db:5432/app"
    );
    // Hosts with ports and plain URLs are not userinfo.
    for plain in [
        "https://example.com:8080/path",
        "ssh://git@github.com/x",
        "max_tokens: 100",
    ] {
        assert_eq!(redact(plain).text, plain);
    }
    // n3: a quoted value takes what follows it, so a second pass changes nothing.
    let quoted = redact(r#"password="a b"x next"#);
    assert_eq!(quoted.text, "password=[REDACTED:secret] next");
    let again = redact(&quoted.text);
    assert_eq!(
        (again.text.as_str(), again.secrets.len()),
        (quoted.text.as_str(), 0)
    );
    // Identifiers holding secrets or private spans are recognised for refusal.
    assert!(holds_secret("https://x.test/doc?token=abc123"));
    assert!(holds_secret("ev-<private>me</private>"));
    assert!(holds_secret("sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUV"));
    assert!(!holds_secret("doc:42") && !holds_secret("chat:run-7/event-3"));
    // Keyword-shaped identifiers are refused (documented on the identifier fields, n2).
    for keyed in [
        "secret:42",
        "token:7",
        "api-key:prod-rotation",
        "db_password:1",
    ] {
        assert!(holds_secret(keyed), "{keyed}");
    }
}

#[test]
fn memory_observation_provider_schema_is_provider_safe() {
    let schema = provider_schema();
    let text = schema.to_string();
    for forbidden in [
        "maxLength",
        "minLength",
        "maxItems",
        "minItems",
        "minimum",
        "maximum",
        "access_level",
        "namespace",
        "session",
        "level",
    ] {
        assert!(!text.contains(forbidden), "{forbidden}");
    }
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(
        schema["properties"]["observations"]["items"]["additionalProperties"],
        false
    );
    assert_eq!(
        schema["properties"]["observations"]["items"]["required"],
        json!(["kind", "title", "body", "event_ids"])
    );
    assert_eq!(reservation(1_000, 2_048), 2_000 + 4_096 + 2_048);
}

fn window() -> Vec<String> {
    ["e1", "e2", "e3"].map(String::from).to_vec()
}

#[test]
fn memory_observation_output_validation_distinguishes_empty_malformed_and_valid_skip() {
    let w = window();
    assert_eq!(validate_output("  ", &w, false), Err(OutputError::Empty));
    assert_eq!(
        validate_output(
            r#"{"outcome":"nothing_to_save","observations":[],"summary":null}"#,
            &w,
            false
        ),
        Ok(ValidOutput::NothingToSave)
    );
    let malformed = [
        "not json",
        r#"{"outcome":"save","observations":[],"summary":null}"#,
        r#"{"outcome":"nothing_to_save","observations":[{"kind":"fact","title":"t","body":"b","event_ids":["e1"]}],"summary":null}"#,
        r#"{"outcome":"maybe","observations":[],"summary":null}"#,
        // Model-chosen labels, namespaces or kinds are not part of the schema.
        r#"{"outcome":"save","observations":[{"kind":"fact","title":"t","body":"b","event_ids":["e1"],"access_level":0}],"summary":null}"#,
        r#"{"outcome":"save","observations":[{"kind":"fact","title":"t","body":"b","event_ids":["e1"]}],"summary":null,"namespace":"public"}"#,
        r#"{"outcome":"save","observations":[{"kind":"summary","title":"t","body":"b","event_ids":["e1"]}],"summary":null}"#,
        r#"{"outcome":"save","observations":[{"kind":"fact","title":"t","body":"b","event_ids":["e9"]}],"summary":null}"#,
        r#"{"outcome":"save","observations":[{"kind":"fact","title":"t","body":"b","event_ids":[]}],"summary":null}"#,
        r#"{"outcome":"save","observations":[{"kind":"fact","title":"","body":"b","event_ids":["e1"]}],"summary":null}"#,
        // A summary nobody asked for.
        r#"{"outcome":"save","observations":[{"kind":"fact","title":"t","body":"b","event_ids":["e1"]}],"summary":"s"}"#,
    ];
    for text in malformed {
        assert!(
            matches!(
                validate_output(text, &w, false),
                Err(OutputError::Malformed(_))
            ),
            "{text}"
        );
    }
    let many: Vec<String> = (0..17)
        .map(|_| r#"{"kind":"fact","title":"t","body":"b","event_ids":["e1"]}"#.to_owned())
        .collect();
    let many = format!(
        r#"{{"outcome":"save","observations":[{}],"summary":null}}"#,
        many.join(",")
    );
    assert!(matches!(
        validate_output(&many, &w, false),
        Err(OutputError::Malformed(_))
    ));
    let long = format!(
        r#"{{"outcome":"save","observations":[{{"kind":"fact","title":"t","body":"{}","event_ids":["e1"]}}],"summary":null}}"#,
        "x".repeat(16_385)
    );
    assert!(matches!(
        validate_output(&long, &w, false),
        Err(OutputError::Malformed(_))
    ));
    // Valid output: cited ids come back in window order, deduplicated.
    let valid = validate_output(
        r#"{"outcome":"save","observations":[{"kind":"decision","title":"t","body":"b","event_ids":["e3","e1","e3"]}],"summary":null}"#,
        &w,
        false,
    )
    .unwrap();
    assert_eq!(
        valid,
        ValidOutput::Save(vec![GeneratedObservation {
            kind: GeneratedKind::Decision,
            title: "t".into(),
            body: "b".into(),
            event_ids: vec!["e1".into(), "e3".into()],
        }])
    );
    // A requested summary is required and covers the whole window.
    let missing = r#"{"outcome":"save","observations":[],"summary":null}"#;
    assert!(validate_output(missing, &w, true).is_err());
    let summary = validate_output(
        r#"{"outcome":"save","observations":[],"summary":"All done."}"#,
        &w,
        true,
    )
    .unwrap();
    let ValidOutput::Save(items) = summary else {
        panic!()
    };
    assert_eq!(
        (items[0].kind, items[0].event_ids.clone()),
        (GeneratedKind::Summary, w)
    );
}
