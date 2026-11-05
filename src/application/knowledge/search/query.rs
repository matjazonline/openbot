use crate::app_error::{AppError, AppResult};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExactKnowledgeTerm(String);

impl ExactKnowledgeTerm {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct KnowledgeSearchQuery {
    question: String,
    exact_terms: Vec<ExactKnowledgeTerm>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    query: String,
    #[serde(default)]
    exact_terms: Vec<String>,
}

impl KnowledgeSearchQuery {
    pub fn parse(value: &Value) -> AppResult<Self> {
        let input: Input = serde_json::from_value(value.clone()).map_err(|_| invalid())?;
        if !valid_text(&input.query, 1000)
            || input.exact_terms.len() > 8
            || input.exact_terms.iter().any(|term| !valid_text(term, 64))
        {
            return Err(invalid());
        }
        let exact_terms = input
            .exact_terms
            .into_iter()
            .map(ExactKnowledgeTerm)
            .collect();
        Ok(Self {
            question: input.query,
            exact_terms,
        })
    }

    pub fn question(&self) -> &str {
        &self.question
    }

    pub fn exact_terms(&self) -> Vec<ExactKnowledgeTerm> {
        let mut terms: BTreeSet<_> = self.exact_terms.iter().cloned().collect();
        for token in self
            .question
            .split(|ch: char| !ch.is_alphanumeric() && ch != '-' && ch != '_')
        {
            let token = token.trim_matches(|ch| ch == '-' || ch == '_');
            if token.chars().count() >= 3 && token.chars().any(char::is_numeric) {
                terms.insert(ExactKnowledgeTerm(token.to_owned()));
            }
        }
        terms.into_iter().collect()
    }
}

fn valid_text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && !value.contains('\0') && value.chars().count() <= max
}

fn invalid() -> AppError {
    AppError::BadRequest(
        "Use a question of 1–1000 characters and at most eight exact terms of 1–64 characters; no other fields are accepted".into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn knowledge_search_input_rejects_levels_unknown_fields_and_invalid_bounds() {
        for value in [
            json!({"query":"price", "level":5}),
            json!({"query":"price", "extra":true}),
            json!({"query":""}),
            json!({"query":" ", "exact_terms":[]}),
            json!({"query":"x".repeat(1001)}),
            json!({"query":"price", "exact_terms":vec!["a";9]}),
            json!({"query":"price", "exact_terms":["x".repeat(65)]}),
            json!({"query":"price", "exact_terms":[""]}),
            json!({"query":"price", "exact_terms":null}),
            json!({"query":"bad\u{0000}query"}),
        ] {
            assert!(KnowledgeSearchQuery::parse(&value).is_err(), "{value}");
        }
        assert!(
            KnowledgeSearchQuery::parse(
                &json!({"query":"界".repeat(1000),"exact_terms":["界".repeat(64)]})
            )
            .is_ok()
        );
    }

    #[test]
    fn knowledge_search_exact_terms_extract_codes_unicode_digits_and_deduplicate() {
        let query = KnowledgeSearchQuery::parse(&json!({
            "query":"Precio AX-4410/AX-4401? Заказ ١٢٣ и 12. AX-4410.",
            "exact_terms":["AX-4410", "Product Pro"]
        }))
        .unwrap();
        let terms: Vec<_> = query.exact_terms().into_iter().map(|term| term.0).collect();
        assert_eq!(terms, ["AX-4401", "AX-4410", "Product Pro", "١٢٣"]);
    }

    #[test]
    fn knowledge_search_limit_is_bounded_and_defaults_to_five() {
        assert_eq!(super::super::KnowledgeSearchLimit::default().get(), 5);
        for count in [0, 9, usize::MAX] {
            assert!(super::super::KnowledgeSearchLimit::new(count).is_err());
        }
        assert_eq!(super::super::KnowledgeSearchLimit::new(8).unwrap().get(), 8);
    }
}
