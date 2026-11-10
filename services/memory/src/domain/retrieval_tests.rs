//! `memory_search_` deterministic fixtures for chunking, vector validation, the vector plan and
//! fusion. The counter stands in for the model tokenizer: one token per whitespace word or CJK
//! character, plus 2 special tokens and the prefix/title words, never truncated.
use super::*;
use chrono::TimeZone;

fn tokens(text: &str) -> usize {
    let words = text.split_whitespace().count();
    let cjk = text.chars().filter(|c| (*c as u32) >= 0x3000).count();
    words + cjk + 2
}

fn chunked(title: &str, body: &str) -> Vec<Range<usize>> {
    chunk(body, |range| Ok(tokens(&passage_input(title, body, range)))).unwrap()
}

fn check_parts(title: &str, body: &str, parts: &[Range<usize>]) {
    let mut previous = 0;
    for part in parts {
        assert!(part.start >= previous, "parts never overlap: {parts:?}");
        assert!(body.is_char_boundary(part.start) && body.is_char_boundary(part.end));
        assert!(tokens(&passage_input(title, body, part.clone())) <= MAX_INPUT_TOKENS);
        previous = part.end;
    }
    // Every word of the body is in some part: nothing is silently dropped.
    let covered: usize = parts
        .iter()
        .map(|part| body[part.clone()].split_whitespace().count())
        .sum();
    assert_eq!(covered, body.split_whitespace().count());
}

#[test]
fn memory_search_chunking_keeps_short_notes_whole_and_blank_bodies_as_the_title() {
    let one = |range: Range<usize>| std::iter::once(range).collect::<Vec<_>>();
    assert_eq!(chunked("Title", "one short note."), one(0..15));
    assert_eq!(chunked("Title", ""), one(0..0));
    assert_eq!(chunked("Title", "  \n\n "), one(0..0));
    let body = "first paragraph.\n\nsecond paragraph.";
    assert_eq!(
        chunked("T", body),
        one(0..body.len()),
        "small paragraphs pack together"
    );
}

#[test]
fn memory_search_chunking_splits_long_notes_by_paragraph_sentence_and_token_cuts() {
    let paragraph = "alpha beta gamma delta. ".repeat(30); // 120 words
    let body = [paragraph.as_str(); 6].join("\n\n");
    let parts = chunked("Release notes", &body);
    assert!(parts.len() >= 3, "{parts:?}");
    check_parts("Release notes", &body, &parts);
    for part in &parts {
        assert!(tokens(&passage_input("Release notes", &body, part.clone())) <= TARGET_PART_TOKENS);
    }
    // One sentence of 700 words: cut by token count, never truncated.
    let sentence = "word ".repeat(700);
    let parts = chunked("T", &sentence);
    assert!(parts.len() >= 3);
    check_parts("T", &sentence, &parts);
    // CJK text without spaces is cut on character boundaries.
    let cjk = "記憶検索のテスト文です".repeat(80);
    let parts = chunked("日本語", &cjk);
    assert!(parts.len() >= 2);
    for part in &parts {
        assert!(tokens(&passage_input("日本語", &cjk, part.clone())) <= MAX_INPUT_TOKENS);
    }
    assert_eq!(parts.first().unwrap().start, 0);
    assert_eq!(parts.last().unwrap().end, cjk.len());
}

#[test]
fn memory_search_chunking_refuses_instead_of_truncating() {
    // A title that alone exceeds the hard limit cannot be indexed.
    let title = "t ".repeat(600);
    assert_eq!(
        chunk("body", |range| Ok(tokens(&passage_input(
            &title, "body", range
        )))),
        Err(ChunkError::PartOverLimit)
    );
    // More than 128 parts is refused, not cut short.
    let body = "w ".repeat(MAX_PARTS * 400);
    assert_eq!(
        chunk(&body, |range| Ok(tokens(&passage_input("T", &body, range)))),
        Err(ChunkError::TooManyParts)
    );
    // A counter that cannot fit even one character is a bug that fails indexing.
    assert_eq!(
        chunk("abc", |range| Ok(if range.is_empty() { 3 } else { 10_000 })),
        Err(ChunkError::PartOverLimit)
    );
    // Counter failures propagate.
    assert_eq!(
        chunk("abc", |_| Err(ChunkError::Counter)),
        Err(ChunkError::Counter)
    );
}

#[test]
fn memory_search_chunking_gives_long_titles_room_for_text() {
    let title = "heading ".repeat(250);
    let body = "word ".repeat(16_000);
    let parts = chunked(&title, &body);
    assert!(parts.len() <= MAX_PARTS);
    check_parts(&title, &body, &parts);
}

#[test]
fn memory_search_vectors_are_validated_and_normalized() {
    let mut raw = vec![0.0f32; DIMENSIONS];
    raw[0] = 3.0;
    raw[1] = 4.0;
    let unit = normalize(raw).unwrap();
    assert!((unit[0] - 0.6).abs() < 1e-6 && (unit[1] - 0.8).abs() < 1e-6);
    assert_eq!(normalize(vec![0.0; DIMENSIONS]), Err(InvalidVector));
    assert_eq!(normalize(vec![1.0; DIMENSIONS - 1]), Err(InvalidVector));
    assert_eq!(normalize(vec![1.0; DIMENSIONS + 1]), Err(InvalidVector));
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut vector = vec![1.0; DIMENSIONS];
        vector[7] = bad;
        assert_eq!(normalize(vector), Err(InvalidVector));
    }
}

#[test]
fn memory_search_vector_plan_retries_full_scans_and_sends_short_ones_to_the_exact_scan() {
    let plan = VectorPlan::new(20);
    assert_eq!(plan.leg_candidates, 80);
    assert_eq!(plan.nearest_parts, 160);
    assert_eq!(plan.ef_search(), 160);
    assert_eq!(VectorPlan::new(100).leg_candidates, 200, "bounded at 200");
    assert_eq!(VectorPlan::new(1).ef_search(), 40);
    assert_eq!(VectorPlan::new(100).ef_search(), 400);
    assert_eq!(plan.next(80, 160), VectorStep::Complete);
    // A full scan with too few distinct observations asks for 4× the parts.
    let VectorStep::Retry(retry) = plan.next(30, 160) else {
        panic!("expected a retry")
    };
    assert_eq!(retry.nearest_parts, 640);
    // A scan that returned fewer parts than its limit is inconclusive: the exact scan decides.
    assert_eq!(plan.next(30, 159), VectorStep::Exact);
    assert_eq!(plan.next(0, 0), VectorStep::Exact);
    // Retries are bounded by 128 parts per candidate.
    let mut step = plan;
    let mut rounds = 0;
    while let VectorStep::Retry(next) = step.next(1, step.nearest_parts) {
        step = next;
        rounds += 1;
    }
    assert_eq!(step.nearest_parts, 80 * MAX_PARTS);
    assert!(rounds <= 4);
}

#[test]
fn memory_search_fusion_is_reciprocal_rank_with_deterministic_ties() {
    let at = |minute| Utc.with_ymd_and_hms(2026, 11, 8, 10, minute, 0).unwrap();
    let id = |n: u128| Uuid::from_u128(n);
    let ranked = |n, minute| Ranked {
        id: id(n),
        occurred_at: at(minute),
    };
    // 1 is in both legs; 2 and 3 tie on score (rank 1 in one leg each), newer first; 4 and 5
    // tie on score and time, lower id first.
    let keyword = [ranked(2, 1), ranked(1, 0), ranked(4, 5)];
    let semantic = [ranked(3, 2), ranked(1, 0), ranked(5, 5)];
    let fused = fuse(&keyword, &semantic);
    let order: Vec<u128> = fused.iter().map(|(r, _)| r.id.as_u128()).collect();
    assert_eq!(order, vec![1, 3, 2, 4, 5]);
    assert!((fused[0].1 - 2.0 / 62.0).abs() < 1e-12);
    assert!((fused[1].1 - 1.0 / 61.0).abs() < 1e-12);
    assert!(fuse(&[], &[]).is_empty());
    // An exact identifier that only the keyword leg finds still ranks.
    let fused = fuse(&[ranked(9, 0)], &[ranked(8, 0), ranked(7, 0)]);
    assert!(fused.iter().any(|(r, _)| r.id == id(9)));
}

#[test]
fn memory_search_snippets_are_bounded_at_character_boundaries() {
    assert_eq!(snippet("  short  "), "short");
    let long = "é".repeat(400);
    let cut = snippet(&long);
    assert!(cut.len() <= SNIPPET_BYTES && cut.len() >= SNIPPET_BYTES - 1);
    assert!(long.starts_with(&cut));
}

#[test]
fn memory_search_identifier_tokens_come_from_the_raw_query() {
    assert_eq!(identifier_tokens("where is PgStore used?"), vec!["PgStore"]);
    assert_eq!(
        identifier_tokens("which idempotency_key do I send"),
        vec!["idempotency_key"]
    );
    assert_eq!(
        identifier_tokens("what is in foo.bar.baz?"),
        vec!["foo.bar.baz"]
    );
    assert_eq!(
        identifier_tokens("who gets mail at ops@example.com."),
        vec!["ops@example.com"]
    );
    assert_eq!(
        identifier_tokens("why ERR_CONN_RESET happens"),
        vec!["ERR_CONN_RESET"]
    );
    assert_eq!(identifier_tokens("error on v1.2.3"), vec!["v1.2.3"]);
    assert_eq!(
        identifier_tokens("see INC-48213, then /var/log"),
        vec!["INC-48213", "/var/log"]
    );
    // Ordinary words, capitalised sentence starts, a lone "I" and punctuation are not ids.
    for plain in [
        "Where is the thing and what do I do?",
        "a the of to",
        "Hello, world!",
        "--- ...",
        // Capitalised words are not identifiers (review p4-code-r2 F1a).
        "how do I reset IT passwords",
        "HOW DO I RESET MY PASSWORD",
        "NASA API",
    ] {
        assert!(identifier_tokens(plain).is_empty(), "{plain}");
    }
    let many: String = (0..40).map(|n| format!("id{n} ")).collect();
    assert_eq!(identifier_tokens(&many).len(), MAX_IDENTIFIER_TOKENS);
    assert_eq!(identifier_tokens("x1 x1 x1"), vec!["x1"]);
}
