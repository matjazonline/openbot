//! `goal_unit_` (Plan08 03A): a goal is a unit with a kind and a target date, created with its
//! sponsors in one transaction; the root stays standing and only a goal has a target date.
use super::*;
use crate::application::ports::department::GoalWrite;
use crate::entities::department::DepartmentKind;

fn date(value: &str) -> chrono::NaiveDate {
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").unwrap()
}

impl Departments {
    fn goal(
        &self,
        name: &str,
        target: Option<&str>,
        sponsors: Vec<PrincipalId>,
    ) -> DepartmentWrite {
        DepartmentWrite {
            goal: Some(GoalWrite {
                target_date: target.map(date),
                sponsors,
            }),
            ..self.write(name, None)
        }
    }

    async fn edit(&self, department: DepartmentId, edit: DepartmentEdit) -> AppResult<Department> {
        self.use_cases
            .update_department(self.owner, self.company, department, edit)
            .await
    }

    async fn sponsors(&self, department: DepartmentId) -> Vec<(Uuid, bool)> {
        sqlx::query_as(
            "SELECT principal_id, decider FROM department_roles \
              WHERE company_id = $1 AND department_id = $2 AND role = 'sponsor' ORDER BY decider DESC",
        )
        .bind(self.company)
        .bind(department.as_uuid())
        .fetch_all(self.pool())
        .await
        .unwrap()
    }
}

fn refused_with(result: &AppResult<Department>, code: &str) -> bool {
    matches!(result, Err(AppError::BadRequest(message)) if message.starts_with(code))
}

#[tokio::test]
async fn goal_unit_kind_and_target_date_round_trip() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let goal = f
        .use_cases
        .create_department(
            f.owner,
            f.company,
            f.goal(
                "Launch X",
                Some("2027-03-31"),
                vec![f.owner_principal, f.member_principal],
            ),
            None,
        )
        .await
        .unwrap();
    assert_eq!(goal.kind, DepartmentKind::Goal);
    assert_eq!(goal.target_date, Some(date("2027-03-31")));
    assert_eq!(
        f.sponsors(goal.id).await,
        vec![
            (f.owner_principal.as_uuid(), true),
            (f.member_principal.as_uuid(), false)
        ],
        "the first sponsor decides"
    );
    let stored = f
        .use_cases
        .visible_department(f.owner, f.company, goal.id)
        .await
        .unwrap();
    assert_eq!(stored.department.kind, DepartmentKind::Goal);
    assert_eq!(stored.department.target_date, Some(date("2027-03-31")));
    let moved = f
        .edit(
            goal.id,
            DepartmentEdit {
                target_date: Some(Some(date("2027-06-30"))),
                ..DepartmentEdit::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(moved.target_date, Some(date("2027-06-30")));
    let standing = f
        .edit(
            goal.id,
            DepartmentEdit {
                kind: Some(DepartmentKind::Standing),
                target_date: Some(None),
                ..DepartmentEdit::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        (standing.kind, standing.target_date),
        (DepartmentKind::Standing, None)
    );
    let plain = f.create("Support", None).await;
    assert_eq!(
        (plain.kind, plain.target_date),
        (DepartmentKind::Standing, None)
    );
}

#[tokio::test]
async fn goal_unit_target_date_on_standing_is_refused() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let plain = f.create("Sales", None).await;
    let refused = f
        .edit(
            plain.id,
            DepartmentEdit {
                target_date: Some(Some(date("2027-01-01"))),
                ..DepartmentEdit::default()
            },
        )
        .await;
    assert!(
        refused_with(&refused, "department.target_date_goal_only"),
        "{refused:?}"
    );
    let goal = f
        .use_cases
        .create_department(
            f.owner,
            f.company,
            f.goal("Cut churn", Some("2027-01-01"), vec![]),
            None,
        )
        .await
        .unwrap();
    // Becoming standing while keeping the date is refused; the stored row is unchanged.
    let refused = f
        .edit(
            goal.id,
            DepartmentEdit {
                kind: Some(DepartmentKind::Standing),
                ..DepartmentEdit::default()
            },
        )
        .await;
    assert!(
        refused_with(&refused, "department.target_date_goal_only"),
        "{refused:?}"
    );
    let error = sqlx::query("UPDATE departments SET target_date = '2027-01-01' WHERE id = $1")
        .bind(plain.id.as_uuid())
        .execute(f.pool())
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().and_then(|e| e.constraint()),
        Some("departments_goal_target")
    );
}

#[tokio::test]
async fn goal_unit_root_cannot_be_goal() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let root = f.root_id().await;
    let refused = f
        .edit(
            root,
            DepartmentEdit {
                kind: Some(DepartmentKind::Goal),
                ..DepartmentEdit::default()
            },
        )
        .await;
    assert!(
        refused_with(&refused, "department.root_is_standing"),
        "{refused:?}"
    );
    let error = sqlx::query("UPDATE departments SET kind = 'goal' WHERE id = $1")
        .bind(root.as_uuid())
        .execute(f.pool())
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().and_then(|e| e.constraint()),
        Some("departments_root_standing")
    );
}

/// A sponsor that cannot hold the role rolls the whole create back: no unit, no desk, no roles.
#[tokio::test]
async fn goal_unit_create_assigns_sponsors_in_one_transaction() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let units = || async {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM departments WHERE company_id = $1")
            .bind(f.company)
            .fetch_one(f.pool())
            .await
            .unwrap()
    };
    let before = units().await;
    let refused = f
        .use_cases
        .create_department(
            f.owner,
            f.company,
            f.goal(
                "Doomed",
                None,
                vec![f.owner_principal, PrincipalId::new(Uuid::new_v4())],
            ),
            None,
        )
        .await;
    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(units().await, before);
    let duplicate = f
        .use_cases
        .create_department(
            f.owner,
            f.company,
            f.goal("Twice", None, vec![f.owner_principal, f.owner_principal]),
            None,
        )
        .await;
    assert!(
        refused_with(&duplicate, "department.sponsor_duplicate"),
        "{duplicate:?}"
    );
    assert_eq!(units().await, before);
}
