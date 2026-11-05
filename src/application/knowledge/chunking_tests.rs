use super::*;
fn chunks(text: &str) -> Vec<KnowledgeChunk> {
    chunk_document(text, |text| Ok(text.chars().count())).unwrap()
}
#[test]
fn knowledge_chunk_short_empty_and_paragraph_packing() {
    for text in ["", "short text"] {
        let result = chunks(text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].body, text);
    }
    let result = chunks("first paragraph\n\nsecond paragraph");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].body, "first paragraph\n\nsecond paragraph");
}
#[test]
fn knowledge_chunk_long_unbroken_unicode_has_no_overlap_or_loss_and_a_hard_limit() {
    let text = "é字🙂x".repeat(300);
    let result = chunks(&text);
    assert!(result.len() > 1);
    assert_eq!(
        result
            .iter()
            .map(|chunk| chunk.body.as_str())
            .collect::<String>(),
        text
    );
    assert!(
        result
            .iter()
            .all(|chunk| chunk.embedding_text().chars().count() <= MAX_EMBEDDING_TOKENS)
    );
}
#[test]
fn knowledge_chunk_heading_paths_respect_fenced_code_and_setext() {
    let result = chunks(
        "# Sales\nOverview\n\n## Discounts\nFloor prices\n\n```md\n# A code comment\n```\n\nOther\n=====\nFinal",
    );
    assert_eq!(result[0].heading.as_deref(), Some("Sales"));
    assert_eq!(result[1].heading.as_deref(), Some("Sales / Discounts"));
    assert!(result[1].body.contains("# A code comment"));
    assert_eq!(result.last().unwrap().heading.as_deref(), Some("Other"));
}
#[test]
fn knowledge_chunk_maximum_sizes_and_heading_amplification_reject_early() {
    assert!(
        chunk_document(
            &format!("# {}", "x".repeat(MAX_HEADING_TOKENS + 1)),
            |text| Ok(text.chars().count())
        )
        .is_err()
    );
    assert!(
        chunk_document(&"# x\n".repeat(MAX_CHUNKS_PER_FILE + 1), |text| Ok(text
            .chars()
            .count()))
        .is_err()
    );
    assert!(
        chunk_document(&"x".repeat(MAX_KNOWLEDGE_FILE_BYTES + 1), |text| Ok(text
            .chars()
            .count()))
        .is_err()
    );
    let result = chunks(&"x".repeat(MAX_EMBEDDING_TOKENS));
    assert_eq!(result.len(), 1);
    let result = chunks(&"x".repeat(MAX_EMBEDDING_TOKENS + 1));
    assert_eq!(result.len(), 2);
}

#[test]
fn knowledge_chunk_skipped_heading_levels_replace_the_correct_path_component() {
    let result = chunks("# Parent\nIntro\n### First\nOne\n### Second\nTwo\n## Middle\nThree");
    assert_eq!(result[1].heading.as_deref(), Some("Parent / First"));
    assert_eq!(result[2].heading.as_deref(), Some("Parent / Second"));
    assert_eq!(result[3].heading.as_deref(), Some("Parent / Middle"));
}

#[test]
fn knowledge_chunk_crlf_and_whitespace_blank_lines_split_at_paragraph_boundaries() {
    let first = "a".repeat(300);
    let second = "b".repeat(300);
    let source = format!("{first}\r\n \t\r\n{second}");
    let result = chunks(&source);
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].body, first);
    assert_eq!(result[1].body, second);
}
