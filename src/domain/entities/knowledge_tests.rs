use super::*;
use crate::entities::{
    company_member::CompanyMembership,
    value_objects::{KnowledgeDocumentKey, ModelName, ModelProvider},
};
use std::collections::HashSet;

#[test]
fn knowledge_upload_filename_prefixes_and_case_insensitive_identity_preserve_display() {
    struct Case {
        raw: &'static str,
        level: i16,
        name: &'static str,
    }
    for case in [
        Case {
            raw: "0_sales.md",
            level: 0,
            name: "sales.md",
        },
        Case {
            raw: "9_x.md",
            level: 9,
            name: "x.md",
        },
        Case {
            raw: "5-sales.md",
            level: 5,
            name: "sales.md",
        },
        Case {
            raw: "docs/5_sales.md",
            level: 5,
            name: "sales.md",
        },
        Case {
            raw: "C:\\docs\\5_sales.md",
            level: 5,
            name: "sales.md",
        },
        Case {
            raw: " 5_ Sales.MD ",
            level: 5,
            name: "Sales.MD",
        },
    ] {
        let parsed = KnowledgeFileName::parse(case.raw).unwrap();
        assert_eq!(parsed.level().as_i16(), case.level);
        assert_eq!(parsed.name().as_str(), case.name);
        assert!(parsed.is_supported_text());
    }
    let first = KnowledgeFileName::parse("5-Straße.md").unwrap();
    let second = KnowledgeFileName::parse("5_STRASSE.MD").unwrap();
    assert_eq!(first.name(), second.name());
    assert_eq!(first.name().key(), second.name().key());
    assert_eq!(first.prefixed(), "5_Straße.md");
    let names = HashSet::from([first.name().clone(), second.name().clone()]);
    assert_eq!(names.len(), 1);
    assert_ne!(
        KnowledgeDocumentKey::from("A"),
        KnowledgeDocumentKey::from("a"),
        "raw key wrappers do not invent normalization"
    );
}

#[test]
fn knowledge_upload_filename_rejects_missing_prefix_controls_empty_and_overlong() {
    for raw in [
        "sales.md",
        "10_sales.md",
        "a_sales.md",
        "5 sales.md",
        "5.sales.md",
        "5_",
        "5-",
        "5_  ",
        "٥_sales.md",
        "5_a\0.md",
        "5_a\u{200b}.md",
    ] {
        let error = KnowledgeFileName::parse(raw).unwrap_err().to_string();
        assert!(error.contains("0_sales.md"), "{raw:?}: {error}");
    }
    assert!(KnowledgeFileName::parse(&format!("0_{}.md", "a".repeat(251))).is_err());
    assert!(
        !KnowledgeFileName::parse("0_sales.pdf")
            .unwrap()
            .is_supported_text()
    );
}

#[test]
fn knowledge_upload_permission_matrix_requires_team_and_does_not_widen_levels() {
    for membership in [
        CompanyMembership::Owner,
        CompanyMembership::Admin,
        CompanyMembership::Member,
        CompanyMembership::None,
    ] {
        for actor_level in AccessLevel::ALL {
            for target in AccessLevel::ALL {
                let actor = LevelActor {
                    membership,
                    access_level: actor_level,
                };
                assert_eq!(
                    may_write_document(actor, target).is_ok(),
                    membership.is_team() && target <= actor_level
                );
            }
        }
    }
}

#[test]
fn knowledge_upload_existing_newtypes_keep_raw_comparison_and_canonical_helpers() {
    let raw = ModelProvider::from(" OpenAI ");
    assert_ne!(raw, ModelProvider::from("openai"));
    assert_eq!(ModelProvider::canonical(raw), "openai");
    assert_ne!(ModelName::from("GPT"), ModelName::from("gpt"));
    assert_eq!(ModelName::from("GPT").as_ref(), "GPT");
}
