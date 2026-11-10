//! Shared strict private stdin contract for explicitly selected native Doop tests.
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::io::Read;

macro_rules! identifier {
    ($name:ident) => {
        #[derive(Deserialize, Serialize)]
        #[serde(transparent)]
        pub(super) struct $name(pub(super) String);
    };
}
identifier!(DoopUserId);
identifier!(DoopWorkspaceId);
identifier!(DoopCanvasId);
identifier!(DoopRevision);
identifier!(SourceFingerprint);

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SourceIdentity {
    pub(super) revision: DoopRevision,
    pub(super) fingerprint: SourceFingerprint,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Human {
    pub(super) id: DoopUserId,
    pub(super) email: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Company {
    pub(super) verification_canvas: DoopCanvasId,
    pub(super) workspace: DoopWorkspaceId,
    pub(super) bot: DoopUserId,
    pub(super) token: SecretString,
    pub(super) humans: Vec<Human>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Input {
    pub(super) origin: String,
    pub(super) source: SourceIdentity,
    pub(super) companies: Vec<Company>,
}

pub(super) fn input() -> Input {
    let input = bounded_input(std::io::stdin().lock());
    validate_input(&input);
    input
}

pub(super) fn bounded_input<T: serde::de::DeserializeOwned>(reader: impl Read) -> T {
    let mut encoded = String::new();
    reader.take(65537).read_to_string(&mut encoded).unwrap();
    assert!(encoded.len() <= 65536, "native helper input exceeds 64 KiB");
    serde_json::from_str(&encoded).expect("bounded private setup input")
}

pub(super) fn validate_input(input: &Input) {
    assert_eq!(input.companies.len(), 2);
    assert!(input.source.revision.0 == "01584b34f6cadcf21767127280cc6e320a6d748a");
    assert!(
        input.source.fingerprint.0.len() == 64
            && input
                .source
                .fingerprint
                .0
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
    );
    let origin = url::Url::parse(&input.origin).unwrap();
    let scheme = if std::env::var_os("DOOP_TEST_OPENBOT_CA").is_some() {
        "https"
    } else {
        "http"
    };
    assert!(origin.scheme() == scheme && origin.host_str() == Some("127.0.0.1"));
    assert!(
        origin.username().is_empty()
            && origin.password().is_none()
            && origin.query().is_none()
            && origin.fragment().is_none()
            && origin.path() == "/"
    );
    assert!(input.companies[0].workspace.0 != input.companies[1].workspace.0);
    assert!(input.companies[0].bot.0 != input.companies[1].bot.0);
    for company in &input.companies {
        for id in [
            &company.bot.0,
            &company.workspace.0,
            &company.verification_canvas.0,
        ] {
            assert!(!id.is_empty() && id.len() <= 128);
        }
        assert!(
            !company.token.expose_secret().is_empty()
                && company.token.expose_secret().len() <= 8192
        );
        assert!(!company.humans.is_empty() && company.humans.len() <= 8);
    }
}
