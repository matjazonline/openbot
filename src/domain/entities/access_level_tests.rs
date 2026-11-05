use super::*;

#[test]
fn access_level_numeric_range_and_serde_are_exact() {
    for level in AccessLevel::ALL {
        assert_eq!(AccessLevel::try_from(level.as_i16()), Ok(level));
        let encoded = serde_json::to_value(level).unwrap();
        assert_eq!(encoded, serde_json::json!(level.as_i16()));
        assert_eq!(
            serde_json::from_value::<AccessLevel>(encoded).unwrap(),
            level
        );
        assert_eq!(level.label(), format!("Level {}", level.as_i16()));
    }
    for value in [-1, 10, i16::MIN, i16::MAX] {
        assert!(AccessLevel::try_from(value).is_err());
    }
    for value in [
        serde_json::json!(-1),
        serde_json::json!(10),
        serde_json::json!(0.5),
        serde_json::json!("5"),
        serde_json::Value::Null,
    ] {
        assert!(serde_json::from_value::<AccessLevel>(value).is_err());
    }
}

#[test]
fn access_level_lowest_is_public_for_no_readers_and_orders_every_level() {
    assert_eq!(AccessLevel::lowest([]), AccessLevel::PUBLIC);
    for low in AccessLevel::ALL {
        for high in AccessLevel::ALL {
            assert_eq!(
                AccessLevel::lowest([high, low]),
                if low <= high { low } else { high }
            );
        }
    }
}

#[test]
fn access_level_assignment_admin_matrix_checks_both_current_and_requested() {
    for ceiling in AccessLevel::ALL {
        let actor = LevelActor {
            membership: CompanyMembership::Admin,
            access_level: ceiling,
        };
        for current in AccessLevel::ALL {
            for requested in AccessLevel::ALL {
                let result = may_assign_level(actor, LevelTarget::Assignable(current), requested);
                assert_eq!(
                    result.is_ok(),
                    current <= ceiling && requested <= ceiling,
                    "actor={ceiling:?} current={current:?} requested={requested:?}"
                );
            }
        }
    }
}

#[test]
fn access_level_assignment_owner_fixed_and_nonadministrators_refused() {
    let owner = LevelActor {
        membership: CompanyMembership::Owner,
        access_level: AccessLevel::MAX,
    };
    for current in AccessLevel::ALL {
        for requested in AccessLevel::ALL {
            assert_eq!(
                may_assign_level(owner, LevelTarget::Assignable(current), requested),
                Ok(())
            );
            assert_eq!(
                may_assign_level(owner, LevelTarget::Owner, requested),
                Err(LevelRefusal::OwnerFixed)
            );
        }
    }
    for membership in [CompanyMembership::Member, CompanyMembership::None] {
        let actor = LevelActor {
            membership,
            access_level: AccessLevel::MAX,
        };
        assert_eq!(
            may_assign_level(
                actor,
                LevelTarget::Assignable(AccessLevel::PUBLIC),
                AccessLevel::PUBLIC
            ),
            Err(LevelRefusal::NotAdministrator)
        );
    }
}
