use super::*;

fn write(name: &str, slug: &str) -> DepartmentWrite {
    DepartmentWrite {
        name: name.into(),
        slug: slug.into(),
        ..DepartmentWrite::default()
    }
}

#[test]
fn department_write_normalize_trims_and_canonicalizes() {
    let mut department = DepartmentWrite {
        description: Some("   ".into()),
        charter: Some("  Keep the lights on.  ".into()),
        ..write("  Customer Care  ", " Customer Care ")
    };
    department.normalize().unwrap();
    assert_eq!(department.name, "Customer Care");
    assert_eq!(department.slug, "customer-care");
    assert_eq!(department.description, None);
    assert_eq!(department.charter.as_deref(), Some("Keep the lights on."));
}

#[test]
fn department_write_normalize_refuses_what_the_schema_would() {
    for (name, slug) in [
        ("   ", "ops"),
        ("Ops", "  "),
        ("Ops", "-ops"),
        ("Ops", "ops-"),
        ("Ops", "ops_team"),
        ("Ops", "ops.team"),
        ("Ops", "opérations"),
        // A reserved suffix would make the department desk's address context-only.
        ("Ops", "ops-quiet"),
    ] {
        let result = write(name, slug).normalize();
        assert!(
            matches!(result, Err(AppError::BadRequest(_))),
            "{name:?}/{slug:?}: {result:?}"
        );
    }
    let mut longest = DepartmentWrite {
        charter: Some("x".repeat(MAX_DEPARTMENT_CHARTER_BYTES)),
        ..write("Ops", "ops")
    };
    longest
        .normalize()
        .expect("a charter of exactly 4 KiB fits");
    let mut too_long = DepartmentWrite {
        charter: Some("é".repeat(MAX_DEPARTMENT_CHARTER_BYTES / 2 + 1)),
        ..write("Ops", "ops")
    };
    let result = too_long.normalize();
    assert!(
        matches!(result, Err(AppError::BadRequest(_))),
        "the bound is bytes, not characters: {result:?}"
    );
}

#[test]
fn department_edit_normalize_touches_only_the_fields_it_sets() {
    let mut edit = DepartmentEdit {
        name: Some("  Ops ".into()),
        description: Some(Some("  ".into())),
        charter: Some(None),
        ..DepartmentEdit::default()
    };
    edit.normalize().unwrap();
    assert_eq!(edit.name.as_deref(), Some("Ops"));
    assert_eq!(edit.slug, None, "an absent slug stays absent");
    assert_eq!(
        edit.description,
        Some(None),
        "a blank description clears it"
    );
    assert_eq!(edit.charter, Some(None));
    assert_eq!(edit.parent_id, None);

    let mut bad = DepartmentEdit {
        slug: Some("Not Valid!".into()),
        ..DepartmentEdit::default()
    };
    assert!(matches!(bad.normalize(), Err(AppError::BadRequest(_))));
}
