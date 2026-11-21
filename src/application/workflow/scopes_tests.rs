use super::*;
use uuid::Uuid;

fn unit(parent: Option<DepartmentId>, slug: &str) -> ScopeUnit {
    ScopeUnit {
        id: DepartmentId::new(Uuid::new_v4()),
        parent,
        slug: slug.into(),
    }
}

#[test]
fn run_scopes_resolve_chain_local_writes_and_only_the_acting_leads_direct_children() {
    let root = unit(None, "root");
    let parent = unit(Some(root.id), "parent");
    let local = unit(Some(parent.id), "local");
    let report = unit(Some(local.id), "report");
    let grandchild = unit(Some(report.id), "grandchild");
    let sibling = unit(Some(parent.id), "sibling");
    let agent = PrincipalId::new(Uuid::new_v4());
    let facts = ScopeRoleFacts {
        root: root.clone(),
        leads: vec![
            LeadChildren {
                principal: agent,
                department: local.id,
                children: vec![report.clone()],
            },
            LeadChildren {
                principal: PrincipalId::new(Uuid::new_v4()),
                department: parent.id,
                children: vec![sibling.clone()],
            },
        ],
    };
    let scopes = run_scopes(
        &[local.clone(), parent.clone(), root.clone()],
        agent,
        &facts,
    )
    .unwrap();
    assert_eq!(scopes.personal, agent);
    assert_eq!(scopes.write_unit, Some(local.id));
    assert_eq!(scopes.units.len(), 4);
    assert!(
        scopes
            .units
            .iter()
            .any(|read| read.unit.id == report.id && read.report)
    );
    assert!(
        !scopes
            .units
            .iter()
            .any(|read| [sibling.id, grandchild.id].contains(&read.unit.id))
    );
    let member = run_scopes(
        &[local, parent, root],
        PrincipalId::new(Uuid::new_v4()),
        &facts,
    )
    .unwrap();
    assert_eq!(member.units.len(), 3);
}

#[test]
fn run_scopes_resolve_absent_or_ambiguous_placement_is_readonly_root() {
    let root = unit(None, "root");
    let agent = PrincipalId::new(Uuid::new_v4());
    let facts = ScopeRoleFacts {
        root: root.clone(),
        leads: vec![LeadChildren {
            principal: agent,
            department: root.id,
            children: vec![unit(Some(root.id), "report")],
        }],
    };
    let scopes = run_scopes(&[], agent, &facts).unwrap();
    assert_eq!(scopes.write_unit, None);
    assert_eq!(scopes.units.len(), 1);
    assert_eq!(scopes.units[0].unit, root);
    assert!(!scopes.units[0].own);
    assert!(!scopes.units[0].report);
    assert_eq!(
        run_scopes(std::slice::from_ref(&root), agent, &facts)
            .unwrap()
            .write_unit,
        Some(root.id)
    );
}

#[test]
fn run_scopes_resolve_overlapping_report_and_chain_is_one_unit_with_both_relations() {
    let root = unit(None, "root");
    let local = unit(Some(root.id), "local");
    let agent = PrincipalId::new(Uuid::new_v4());
    let facts = ScopeRoleFacts {
        root: root.clone(),
        leads: vec![LeadChildren {
            principal: agent,
            department: root.id,
            children: vec![local.clone(), local.clone()],
        }],
    };
    let scopes = run_scopes(&[local.clone(), root], agent, &facts).unwrap();
    assert_eq!(scopes.units.len(), 2);
    assert!(
        scopes
            .units
            .iter()
            .any(|read| read.unit.id == local.id && read.own && read.report)
    );
}

#[test]
fn run_scopes_resolve_depth_cycle_chain_metadata_and_read_budget_fail_early() {
    let root = unit(None, "root");
    let agent = PrincipalId::new(Uuid::new_v4());
    let mut facts = ScopeRoleFacts {
        root: root.clone(),
        leads: vec![],
    };
    let mut chain = vec![root.clone()];
    for _ in 1..MAX_DEPARTMENT_DEPTH {
        chain.insert(0, unit(Some(chain[0].id), "nested"));
    }
    assert!(run_scopes(&chain, agent, &facts).is_ok());
    chain.insert(0, unit(Some(chain[0].id), "too-deep"));
    assert!(run_scopes(&chain, agent, &facts).is_err());
    assert!(run_scopes(&[root.clone(), root.clone()], agent, &facts).is_err());
    let mut wrong = unit(Some(root.id), "unsafe\ntext");
    assert!(run_scopes(&[wrong.clone(), root.clone()], agent, &facts).is_err());
    wrong.slug = "safe".into();
    wrong.parent = None;
    assert!(run_scopes(&[wrong, root.clone()], agent, &facts).is_err());
    facts.leads.push(LeadChildren {
        principal: agent,
        department: root.id,
        children: (1..MAX_RUN_READ_UNITS)
            .map(|_| unit(Some(root.id), "report"))
            .collect(),
    });
    assert_eq!(
        run_scopes(std::slice::from_ref(&root), agent, &facts)
            .unwrap()
            .units
            .len(),
        MAX_RUN_READ_UNITS
    );
    facts.leads[0]
        .children
        .push(unit(Some(root.id), "over-limit"));
    assert!(
        matches!(run_scopes(&[root], agent, &facts), Err(AppError::BadRequest(message)) if message == "memory.scope_limit_exceeded")
    );
}
