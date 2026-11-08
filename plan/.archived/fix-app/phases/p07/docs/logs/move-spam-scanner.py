from pathlib import Path
import re,textwrap
old=Path('src/application/services/spam_scanner.rs').read_text()
d=Path('src/adapters/spam_scanner');d.mkdir()
# Preserve the heuristic policy while splitting its existing phases into synchronous helpers.
a=old.index('            let subj_upper');b=old.index('\n        }\n\n        // Body heuristics',a)
subject=textwrap.dedent(old[a:b])
a=old.index('            let body_upper');b=old.index('\n        }\n\n        // Sender address sanity',a)
body=textwrap.dedent(old[a:b])
heuristic='''//! Local scoring policy, unchanged from the former application scanner.
use crate::application::services::spam_scanner::SpamScanResult;
pub(super) struct HeuristicScanner;
impl HeuristicScanner {
    pub(super) fn scan(subject: Option<&str>, from: Option<&str>, body_text: Option<&str>, body_html: Option<&str>) -> SpamScanResult {
        let mut result = SpamScanResult::new();
        if let Some(subject) = subject { score_subject(&mut result, subject); }
        let combined_body = format!("{}\\n{}", body_text.unwrap_or_default(), body_html.unwrap_or_default());
        if !combined_body.trim().is_empty() { score_body(&mut result, &combined_body, body_html); }
        if let Some(from) = from && from.contains("noreply@") && combined_body.contains("click here to reply") {
            result.add(1.0, "No-reply sender with reply request link");
        }
        result
    }
}
fn score_subject(result: &mut SpamScanResult, subj: &str) {
'''+subject+'\n}\nfn score_body(result: &mut SpamScanResult, combined_body: &str, body_html: Option<&str>) {\n'+body+'\n}\n'
(d/'heuristic.rs').write_text(heuristic)
result=old[old.index('#[derive(Debug, Clone, Default'):old.index('/// Stage 1:')]
Path('src/application/services/spam_scanner.rs').write_text('''//! Application-owned spam-scanning boundary; network clients live in adapters.
use async_trait::async_trait;
'''+result+'''/// Borrowed message facts; the scanner neither owns nor logs message bytes.
#[derive(Clone, Copy)]
pub struct SpamScanInput<'a> {
    pub raw_mime: &'a [u8],
    pub subject: Option<&'a str>,
    pub from: Option<&'a str>,
    pub body_text: Option<&'a str>,
    pub body_html: Option<&'a str>,
}
#[async_trait]
pub trait SpamScanner: Send + Sync {
    async fn scan(&self, input: SpamScanInput<'_>) -> SpamScanResult;
}
''')
# Retain the existing heuristic assertion as an adapter test.
a=old.index('    #[test]\n    fn test_heuristic');b=old.index('    #[tokio::test]',a)
(d/'tests.rs').write_text('use super::*;\n'+textwrap.dedent(old[a:b]))
p=Path('src/adapters/mod.rs');p.write_text(p.read_text().replace('pub mod smtp;','pub mod smtp;\npub mod spam_scanner;'))
p=Path('src/application/use_cases/thread/address_grouping_tests.rs');p.write_text(p.read_text().replace('internal_test_config()','internal_test_settings()'))
