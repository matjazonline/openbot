//! Content inspection is ingress classification, not a human read of a stored message.
//! Local scoring policy, unchanged from the former application scanner.
use crate::application::services::spam_scanner::SpamScanResult;
pub(super) struct HeuristicScanner;
impl HeuristicScanner {
    pub(super) fn scan(
        subject: Option<&str>,
        from: Option<&str>,
        body_text: Option<&str>,
        body_html: Option<&str>,
    ) -> SpamScanResult {
        let mut result = SpamScanResult::new();
        if let Some(subject) = subject {
            score_subject(&mut result, subject);
        }
        let combined_body = format!(
            "{}\n{}",
            body_text.unwrap_or_default(),
            body_html.unwrap_or_default()
        );
        if !combined_body.trim().is_empty() {
            score_body(&mut result, &combined_body, body_html);
        }
        if let Some(from) = from
            && from.contains("noreply@")
            && combined_body.contains("click here to reply")
        {
            result.add(1.0, "No-reply sender with reply request link");
        }
        result
    }
}
fn score_subject(result: &mut SpamScanResult, subj: &str) {
    let subj_upper = subj.to_uppercase();

    // High urgency/spam keywords in subject
    let keywords = [
        ("URGENT", 1.5),
        ("CLAIM YOUR REWARD", 3.0),
        ("WINNER", 2.0),
        ("LOTTERY", 3.0),
        ("CASINO", 2.5),
        ("CRYPTO OFFER", 2.5),
        ("FREE GIFT", 2.0),
        ("ACT NOW", 1.5),
        ("BITCOIN", 2.0),
        ("CONGRATULATIONS", 1.5),
        ("100% FREE", 2.5),
        ("MAKE MONEY", 2.5),
    ];

    for (kw, score) in keywords {
        if subj_upper.contains(kw) {
            result.add(score, format!("Subject contains spam keyword '{}'", kw));
        }
    }

    // Excess uppercase ratio check
    if subj.len() > 10 {
        let uppercase_count = subj.chars().filter(|c| c.is_uppercase()).count();
        let ratio = uppercase_count as f64 / subj.len() as f64;
        if ratio > 0.65 {
            result.add(1.5, "Excessive uppercase letters in subject");
        }
    }

    // Punctuation check
    if subj.contains("!!!") || subj.contains("???") || subj.contains("$$$") {
        result.add(1.0, "Excessive punctuation in subject");
    }
}
fn score_body(result: &mut SpamScanResult, combined_body: &str, body_html: Option<&str>) {
    let body_upper = combined_body.to_uppercase();

    // Shortener link detection
    let shorteners = [
        "bit.ly/",
        "tinyurl.com/",
        "t.co/",
        "goo.gl/",
        "ow.ly/",
        "is.gd/",
        "buff.ly/",
    ];
    for shortener in shorteners {
        if combined_body.contains(shortener) {
            result.add(1.5, format!("Contains shortened link '{}'", shortener));
        }
    }

    // Financial/phishing triggers
    if body_upper.contains("SEND BITCOIN TO") || body_upper.contains("WALLET ADDRESS:") {
        result.add(3.0, "Crypto ransom or payment solicitation in body");
    }

    if body_upper.contains("VERIFY YOUR ACCOUNT IMMEDIATELY")
        || body_upper.contains("ACCOUNT SUSPENDED")
    {
        result.add(2.0, "Urgent account verification phishing language");
    }

    // Hidden text trick detection in HTML
    if let Some(html) = body_html {
        let html_lower = html.to_lowercase();
        if html_lower.contains("display:none")
            || html_lower.contains("display: none")
            || html_lower.contains("font-size:0")
            || html_lower.contains("font-size: 0")
        {
            result.add(2.0, "Hidden text elements detected in HTML body");
        }
    }
}
