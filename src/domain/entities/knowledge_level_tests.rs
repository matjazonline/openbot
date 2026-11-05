use super::*;

#[test]
fn knowledge_run_level_release_matrix_never_declassifies() {
    for content in AccessLevel::ALL {
        for destination in AccessLevel::ALL {
            assert_eq!(may_release(content, destination), content <= destination);
        }
    }
}
#[test]
fn knowledge_run_level_audience_is_the_lowest_reader_or_public() {
    assert_eq!(audience_level([]), AccessLevel::PUBLIC);
    assert_eq!(
        audience_level([AccessLevel::L5, AccessLevel::MAX]),
        AccessLevel::L5
    );
    assert_eq!(
        audience_level([AccessLevel::L5, AccessLevel::PUBLIC]),
        AccessLevel::PUBLIC
    );
}
#[test]
fn knowledge_run_level_domain_keeps_content_within_the_admission_ceiling() {
    for ceiling in AccessLevel::ALL {
        for content in AccessLevel::ALL {
            let levels = RunKnowledge::new(ceiling, content);
            assert_eq!(levels.is_ok(), content <= ceiling);
            if let Ok(levels) = levels {
                assert_eq!(levels.ceiling(), ceiling);
                assert_eq!(levels.content(), content);
                assert_eq!(
                    serde_json::to_value(levels).unwrap(),
                    serde_json::json!({"knowledge_ceiling":ceiling.as_i16(),"content_level":content.as_i16()})
                );
            }
        }
    }
}
