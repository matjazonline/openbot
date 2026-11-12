use super::*;
use crate::application::workflow::publication::{DependencySnapshots, freeze};
use crate::application::workflow::registry;
use crate::domain::workflow::{CompanyId, Context, RunId, RunMetadata, StepId, VersionId};
use serde_json::json;
use uuid::Uuid;

fn department(holders: Vec<RoleHolder>) -> RoleDepartment {
    RoleDepartment {
        department: DepartmentId::new(Uuid::new_v4()),
        holders,
    }
}

fn holder(role: DepartmentRole, kind: PrincipalKind) -> RoleHolder {
    RoleHolder {
        role,
        principal: PrincipalId::new(Uuid::new_v4()),
        kind,
    }
}

#[test]
fn workflow_role_resolve_nearest_holder_and_wrong_kind_never_fall_through() {
    for role in DepartmentRole::ALL {
        for need in [RoleNeed::Agent, RoleNeed::People] {
            let kind = if need == RoleNeed::Agent {
                PrincipalKind::Agent
            } else {
                PrincipalKind::Person
            };
            let chain = vec![
                department(vec![]),
                department(vec![holder(role, kind)]),
                department(vec![holder(role, kind)]),
            ];
            let resolved = resolve_role(&chain, role, need).unwrap();
            assert_eq!(resolved.department, chain[1].department);
            assert_eq!(resolved.principals, [chain[1].holders[0].principal]);
            let own = vec![
                department(vec![holder(role, kind)]),
                department(vec![holder(role, kind)]),
            ];
            assert_eq!(
                resolve_role(&own, role, need).unwrap().department,
                own[0].department
            );
            for wrong in [
                PrincipalKind::Agent,
                PrincipalKind::Person,
                PrincipalKind::External,
                PrincipalKind::System,
            ] {
                if wrong == kind {
                    continue;
                }
                let wrong_chain = vec![
                    department(vec![holder(role, wrong)]),
                    department(vec![holder(role, kind)]),
                ];
                assert_eq!(
                    resolve_role(&wrong_chain, role, need),
                    Err(RoleRefusalKind::WrongKind)
                );
            }
        }
    }
}

#[test]
fn workflow_role_resolve_missing_depth_and_cycle_are_bounded() {
    assert_eq!(
        resolve_role(&[], DepartmentRole::Lead, RoleNeed::Agent),
        Err(RoleRefusalKind::NoHolder)
    );
    let mut chain: Vec<_> = (0..MAX_DEPARTMENT_DEPTH)
        .map(|_| department(vec![]))
        .collect();
    assert_eq!(
        resolve_role(&chain, DepartmentRole::Lead, RoleNeed::Agent),
        Err(RoleRefusalKind::NoHolder)
    );
    chain
        .last_mut()
        .unwrap()
        .holders
        .push(holder(DepartmentRole::Lead, PrincipalKind::Agent));
    assert_eq!(
        resolve_role(&chain, DepartmentRole::Lead, RoleNeed::Agent)
            .unwrap()
            .department,
        chain[MAX_DEPARTMENT_DEPTH - 1].department
    );
    chain.push(department(vec![]));
    assert_eq!(
        resolve_role(&chain, DepartmentRole::Lead, RoleNeed::Agent),
        Err(RoleRefusalKind::DepthBound)
    );
    chain.pop();
    chain[1].department = chain[0].department;
    assert_eq!(
        resolve_role(&chain, DepartmentRole::Lead, RoleNeed::Agent),
        Err(RoleRefusalKind::DepthBound)
    );
}

#[test]
fn workflow_role_resolve_cardinality_deduplicates_people_and_refuses_ambiguous_agents() {
    let role = DepartmentRole::Reviewer;
    let two_agents = vec![department(vec![
        holder(role, PrincipalKind::Agent),
        holder(role, PrincipalKind::Agent),
    ])];
    assert_eq!(
        resolve_role(&two_agents, role, RoleNeed::Agent),
        Err(RoleRefusalKind::Ambiguous)
    );
    let mut people: Vec<_> = (0..crate::domain::workflow::TokenAllowance::MAX_REVIEWERS)
        .map(|_| holder(role, PrincipalKind::Person))
        .collect();
    people.push(RoleHolder {
        role,
        principal: people[0].principal,
        kind: PrincipalKind::Person,
    });
    let mut chain = vec![department(people)];
    assert_eq!(
        resolve_role(&chain, role, RoleNeed::People)
            .unwrap()
            .principals
            .len(),
        crate::domain::workflow::TokenAllowance::MAX_REVIEWERS
    );
    chain[0].holders.push(holder(role, PrincipalKind::Person));
    assert_eq!(
        resolve_role(&chain, role, RoleNeed::People),
        Err(RoleRefusalKind::TooMany)
    );
    let mixed = vec![department(vec![
        holder(role, PrincipalKind::Agent),
        holder(role, PrincipalKind::Person),
    ])];
    for need in [RoleNeed::Agent, RoleNeed::People] {
        assert_eq!(
            resolve_role(&mixed, role, need),
            Err(RoleRefusalKind::WrongKind)
        );
    }
}

#[test]
fn workflow_role_admission_input_preparation_uses_the_frozen_requirement_and_sample_refuses_contextless_execution()
 {
    for kind in ["agent.run", "decision.agent", "ai.classify"] {
        let mut source: serde_json::Value =
            serde_json::from_str(&registry::example(kind).unwrap().source).unwrap();
        source["input_schema"] = json!({"type":"object"});
        let inputs = source["steps"]["start"]["with"].as_object_mut().unwrap();
        inputs.remove("agent");
        inputs.insert("role".into(), json!({"literal":"lead"}));
        let bundle = freeze(
            crate::adapters::workflow_source::decode(&source.to_string()).unwrap(),
            CompanyId::new(Uuid::new_v4()),
            VersionId::new(Uuid::new_v4()),
            DependencySnapshots::default(),
            vec![],
        )
        .unwrap();
        let input = json!({});
        let params = json!({});
        let outputs = Default::default();
        let context = Context {
            input: &input,
            params: &params,
            step_outputs: &outputs,
            run: RunMetadata {
                run_id: RunId::new(Uuid::new_v4()),
                parent_run_id: None,
                deadline: None,
            },
        };
        let step = StepId::parse("start").unwrap();
        let prepared = bundle.prepare_step_inputs(&step, &context).unwrap();
        assert_eq!(prepared.value()["role"], "lead");
        assert!(prepared.value().get("agent").is_none());
        let error = refuse_unresolved_sample_role(&bundle, &step).unwrap_err();
        assert!(
            matches!(error, crate::app_error::AppError::WorkflowRole(ref refusal)
            if refusal.step == step && refusal.role == DepartmentRole::Lead && refusal.kind == RoleRefusalKind::MissingDepartment)
        );
    }
}

#[test]
fn workflow_role_resolve_refusal_details_are_versioned_bounded_and_contain_no_principals() {
    use crate::entities::department::RoleAdmissionRefusal;
    let refusal = RoleAdmissionRefusal::new(
        StepId::parse("draft").unwrap(),
        DepartmentRole::Lead,
        RoleRefusalKind::NoHolder,
    );
    let value = serde_json::to_value(&refusal).unwrap();
    assert_eq!(
        value,
        json!({"format":"workflow-role-refusal-v1","step":"draft","role":"lead","kind":"no_holder"})
    );
    assert_eq!(
        serde_json::from_value::<RoleAdmissionRefusal>(value.clone()).unwrap(),
        refusal
    );
    let mut wrong = value.clone();
    wrong["format"] = json!("workflow-role-refusal-v2");
    assert!(serde_json::from_value::<RoleAdmissionRefusal>(wrong).is_err());
    let mut extra = value;
    extra["principal"] = json!(Uuid::new_v4());
    assert!(serde_json::from_value::<RoleAdmissionRefusal>(extra).is_err());
    for kind in [
        RoleRefusalKind::NoHolder,
        RoleRefusalKind::WrongKind,
        RoleRefusalKind::Ambiguous,
        RoleRefusalKind::DepthBound,
        RoleRefusalKind::TooMany,
        RoleRefusalKind::Unavailable,
        RoleRefusalKind::MissingDepartment,
    ] {
        assert!(
            kind.code().len() <= 64,
            "the existing refusal-code bound never moves"
        );
    }
}
