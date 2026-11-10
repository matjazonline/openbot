use super::*;

fn level(value: i64) -> AccessLevel {
    AccessLevel::new(value).unwrap()
}

fn inputs(ceiling: Option<i64>, consumed: Consumed) -> LabelInputs {
    LabelInputs {
        producing_ceiling: ceiling.map(level),
        consumed,
        service_sources: vec![],
        external_sources: vec![],
        expected: None,
    }
}

fn exact(value: i64) -> Consumed {
    Consumed::Exact {
        level: level(value),
    }
}

#[test]
fn memory_access_level_range_is_exactly_zero_through_nine() {
    assert_eq!(AccessLevel::all().count(), 10);
    for value in 0..=9 {
        assert_eq!(level(value).value() as i64, value);
    }
    for value in [-1, 10, 255, i64::MAX, i64::MIN] {
        assert_eq!(AccessLevel::new(value), Err(LevelOutOfRange(value)));
    }
    assert!(serde_json::from_str::<AccessLevel>("10").is_err());
    assert!(serde_json::from_str::<AccessLevel>("-1").is_err());
    assert_eq!(serde_json::from_str::<AccessLevel>("6").unwrap(), level(6));
}

#[test]
fn memory_access_level_ceiling_reads_are_ordered_across_all_ten_levels() {
    for ceiling in AccessLevel::all() {
        for record in AccessLevel::all() {
            assert_eq!(
                record.visible_under(ceiling),
                record.value() <= ceiling.value()
            );
        }
    }
    let visible: Vec<_> = AccessLevel::all()
        .filter(|record| record.visible_under(level(6)))
        .map(AccessLevel::value)
        .collect();
    assert_eq!(visible, vec![0, 1, 2, 3, 4, 5, 6]);
}

#[test]
fn memory_access_level_contract_table_rows() {
    // Ceiling 6, consumed 4 → 4.
    let label = derive_label(&inputs(Some(6), exact(4)), Some(level(9))).unwrap();
    assert_eq!(label.level(), level(4));
    assert_eq!(label.watermark_kind(), WatermarkKind::Exact);
    // Ceiling 6, consumed 6 → 6.
    let label = derive_label(&inputs(Some(6), exact(6)), Some(level(9))).unwrap();
    assert_eq!(label.level(), level(6));
    // Ceiling 6, unknown → 6 via the ceiling fallback.
    let label = derive_label(&inputs(Some(6), Consumed::Unknown), Some(level(9))).unwrap();
    assert_eq!(label.level(), level(6));
    assert_eq!(label.watermark_kind(), WatermarkKind::CeilingFallback);
    assert_eq!(label.watermark_level(), label.producing_ceiling());
    // Ceiling 6, consumed 7 → refused.
    assert_eq!(
        derive_label(&inputs(Some(6), exact(7)), Some(level(9))),
        Err(LabelRefusal::ConsumedExceedsCeiling {
            consumed: level(7),
            ceiling: level(6)
        })
    );
}

#[test]
fn memory_access_level_inherited_level_six_context_with_public_recall_saves_at_six() {
    // The context already consumed level-6 history; the latest recalled notes were public.
    let mut context = inputs(Some(6), exact(6));
    context.service_sources = vec![level(0), level(0)];
    assert_eq!(
        derive_label(&context, Some(level(9))).unwrap().level(),
        level(6)
    );
}

#[test]
fn memory_access_level_sources_raise_but_never_lower_the_label() {
    let mut context = inputs(Some(6), exact(2));
    context.service_sources = vec![level(5)];
    context.external_sources = vec![level(3)];
    let label = derive_label(&context, Some(level(9))).unwrap();
    assert_eq!(label.level(), level(5));
    assert_eq!(label.source_max_level(), level(5));
    context.external_sources = vec![level(7)];
    assert_eq!(
        derive_label(&context, Some(level(9))).unwrap_err().reason(),
        "source_above_ceiling"
    );
}

#[test]
fn memory_access_level_missing_trusted_ceiling_refuses_rather_than_saving_public() {
    for consumed in [Consumed::Unknown, exact(0)] {
        assert_eq!(
            derive_label(&inputs(None, consumed), Some(level(9))),
            Err(LabelRefusal::MissingTrustedCeiling)
        );
    }
}

#[test]
fn memory_access_level_proven_public_input_saves_at_zero() {
    let label = derive_label(&inputs(Some(6), exact(0)), Some(level(0))).unwrap();
    assert_eq!(label.level(), AccessLevel::PUBLIC);
}

#[test]
fn memory_access_level_forged_lower_expected_label_is_refused() {
    let mut context = inputs(Some(6), Consumed::Unknown);
    context.expected = Some(level(2));
    assert_eq!(
        derive_label(&context, Some(level(9))).unwrap_err().reason(),
        "inconsistent_classification"
    );
    context.expected = Some(level(6));
    assert!(derive_label(&context, Some(level(9))).is_ok());
}

#[test]
fn memory_access_level_write_grant_bounds_the_label() {
    assert_eq!(
        derive_label(&inputs(Some(6), exact(5)), Some(level(4))).unwrap_err(),
        LabelRefusal::AboveWriteGrant { level: level(5) }
    );
    assert_eq!(
        derive_label(&inputs(Some(6), exact(0)), None).unwrap_err(),
        LabelRefusal::AboveWriteGrant { level: level(0) }
    );
}

#[test]
fn memory_access_level_replayed_inputs_derive_the_identical_label() {
    // The label is a pure function of its hashed inputs: a replay cannot relabel.
    let mut context = inputs(Some(6), Consumed::Unknown);
    context.service_sources = vec![level(1)];
    let first = derive_label(&context, Some(level(9))).unwrap();
    for _ in 0..3 {
        assert_eq!(derive_label(&context, Some(level(9))).unwrap(), first);
    }
    // A later lower audience ceiling is a different input, never the same logical request.
    let mut lowered = context.clone();
    lowered.producing_ceiling = Some(level(3));
    assert_ne!(lowered, context);
}

#[test]
fn memory_access_level_correction_never_declassifies() {
    let lower = derive_label(&inputs(Some(6), exact(1)), Some(level(9))).unwrap();
    assert_eq!(corrected_level(level(6), lower), level(6));
    let higher = derive_label(&inputs(Some(8), exact(8)), Some(level(9))).unwrap();
    assert_eq!(corrected_level(level(6), higher), level(8));
}

#[test]
fn memory_access_level_exposed_level_is_the_returned_maximum() {
    assert_eq!(exposed_level([]), AccessLevel::PUBLIC);
    assert_eq!(exposed_level([level(0), level(4), level(6)]), level(6));
}
