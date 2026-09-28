use super::*;
use compiler::{Diagnostic, MAX_SOURCE_BYTES, SourceDecoder, rebase_workflow_id};

struct NeverDecode;
impl SourceDecoder for NeverDecode {
    fn decode(&self, _: &str) -> Result<compiler::DecodedSource, Diagnostic> {
        panic!("preflight must reject before decoder or compiler work")
    }
}

#[test]
fn workflow_templates_catalogue_preflights_all_entries_and_aggregate_before_decoding() {
    let example = registry::example("data.map").unwrap();
    for (field, text) in [
        ("title", "x".repeat(MAX_TITLE_BYTES + 1)),
        ("description", "x".repeat(MAX_DESCRIPTION_BYTES + 1)),
        ("source", "x".repeat(MAX_SOURCE_BYTES + 1)),
    ] {
        let mut bad = offer(&example, 1);
        bad.id = TemplateId::parse("other").unwrap();
        match field {
            "title" => bad.title = &text,
            "description" => bad.description = &text,
            _ => bad.source = &text,
        }
        assert!(matches!(
            TemplateCatalogue::build(&[offer(&example, 1), bad], &NeverDecode),
            Err(TemplateError::Limit(_))
        ));
    }
    assert!(matches!(
        TemplateCatalogue::build(&[offer(&example, 1), offer(&example, 2)], &NeverDecode),
        Err(TemplateError::Duplicate(_))
    ));
    let many: Vec<_> = (0..=MAX_TEMPLATES).map(|_| offer(&example, 1)).collect();
    assert!(matches!(
        TemplateCatalogue::build(&many, &NeverDecode),
        Err(TemplateError::Limit("template count"))
    ));
    let large = "x".repeat(MAX_SOURCE_BYTES);
    let offers: Vec<_> = (0..9)
        .map(|i| {
            let mut o = offer(&example, 1);
            o.id = TemplateId::parse(format!("template_{i}")).unwrap();
            o.source = &large;
            o
        })
        .collect();
    assert!(matches!(
        TemplateCatalogue::build(&offers, &NeverDecode),
        Err(TemplateError::Limit("aggregate catalogue bytes"))
    ));
    assert!(TemplateRevision::new(0).is_none());
    assert!(crate::domain::workflow::DraftRevision::new(0).is_none());
    assert!(
        CopyTemplateRequest::new(
            company(10),
            actor(),
            TemplateId::parse("starter").unwrap(),
            TemplateRevision::new(1).unwrap(),
            workflow(0)
        )
        .is_err()
    );
    assert!(
        CopyTemplateRequest::new(
            company(0),
            actor(),
            TemplateId::parse("starter").unwrap(),
            TemplateRevision::new(1).unwrap(),
            workflow(20)
        )
        .is_err()
    );
}

#[test]
fn workflow_templates_offered_source_errors_remain_located_and_actionable() {
    let example = registry::example("data.map").unwrap();
    let invalid = [
        "format_version: [".to_owned(),
        example.source.replace("data.map", "unknown.step"),
        example.source.replace("$end", "absent"),
        example.source.replace(
            "\"input_schema\":true",
            "\"input_schema\":{\"type\":\"nonsense\"}",
        ),
    ];
    for source in invalid {
        let mut entry = offer(&example, 1);
        entry.source = &source;
        let Err(TemplateError::Validation(error)) =
            TemplateCatalogue::build(&[entry], &WorkflowSourceDecoder)
        else {
            panic!("expected located validation failure for {source}");
        };
        assert!(!error.code.is_empty());
        assert!(!error.message.is_empty());
        assert!(error.span.line > 0);
        assert!(error.span.start <= source.len());
        if error.code != "source.yaml" {
            assert!(!error.field_path.is_empty());
        }
    }
}

fn yaml_source(identity: &str) -> String {
    format!(
        r#"# root text 00000000-0000-0000-0000-000000000001 and Unicode é
format_version: 1
workflow_id: {identity} # keep this comment
input_schema: true
parameter_schema: true
output_schema: true
resources: []
entry: start
steps:
  start:
    type: data.map
    with:
      value:
        literal: 00000000-0000-0000-0000-000000000001
    routes: {{success: $end}}
limits: {{max_steps: 100, max_context_bytes: 65536}}
"#
    )
}

#[test]
fn workflow_templates_rebase_preserves_literal_comment_quotes_and_child_pins() {
    let id = workflow(1).to_string();
    for scalar in [id.clone(), format!("'{id}'"), format!("\"{id}\"")] {
        let source = yaml_source(&scalar);
        let result = rebase_workflow_id(&WorkflowSourceDecoder, &source, workflow(20)).unwrap();
        let expected = source.replacen(
            &format!("workflow_id: {scalar}"),
            &format!(
                "workflow_id: {}",
                scalar.replace(&id, &workflow(20).to_string())
            ),
            1,
        );
        assert_eq!(result, expected);
        assert!(result.contains(&format!("literal: {id}")));
        assert!(result.contains(&format!("# root text {id}")));
    }
    let mut child = registry::example("workflow.call").unwrap();
    // A root ID also appearing as a child pin must remain unchanged there.
    child.source = child.source.replace(&workflow(2).to_string(), &id);
    let rebased = rebase_workflow_id(&WorkflowSourceDecoder, &child.source, workflow(20)).unwrap();
    let parsed =
        compiler::parse_workflow(&decode(&rebased).unwrap(), VersionId::new(Uuid::nil())).unwrap();
    assert_eq!(parsed.definition.workflow_id, workflow(20));
    assert!(rebased.contains(&format!("\"child_workflow_id\":\"{id}\"")));
    assert!(rebase_workflow_id(&WorkflowSourceDecoder, &child.source, workflow(0)).is_err());
    assert!(rebase_workflow_id(&WorkflowSourceDecoder, &child.source, workflow(1)).is_err());
}

#[test]
fn workflow_templates_rebase_enforces_exact_source_byte_boundary() {
    let mut source = yaml_source(&workflow(1).to_string());
    source.push('#');
    source.push_str(&"x".repeat(MAX_SOURCE_BYTES - source.len()));
    assert_eq!(source.len(), MAX_SOURCE_BYTES);
    let output = rebase_workflow_id(&WorkflowSourceDecoder, &source, workflow(20)).unwrap();
    assert_eq!(output.len(), MAX_SOURCE_BYTES);
    source.push('x');
    assert_eq!(
        rebase_workflow_id(&WorkflowSourceDecoder, &source, workflow(20))
            .unwrap_err()
            .code,
        "source.limit"
    );
    // UUID's accepted compact form grows when rendered canonically.
    let mut compact = yaml_source("\"00000000000000000000000000000001\"");
    compact.push('#');
    compact.push_str(&"x".repeat(MAX_SOURCE_BYTES - compact.len()));
    assert_eq!(
        rebase_workflow_id(&WorkflowSourceDecoder, &compact, workflow(20))
            .unwrap_err()
            .code,
        "source.limit"
    );
}

#[test]
fn workflow_templates_reject_nil_template_root() {
    let mut example = registry::example("data.map").unwrap();
    example.source = example
        .source
        .replace(&workflow(1).to_string(), &workflow(0).to_string());
    example.dependencies = BTreeMap::from([(workflow(0), Default::default())]);
    let Err(TemplateError::Validation(error)) =
        TemplateCatalogue::build(&[offer(&example, 1)], &WorkflowSourceDecoder)
    else {
        panic!("nil root must fail");
    };
    assert_eq!(error.code, "template.identity");
    assert_eq!(error.field_path.as_ref(), "/workflow_id");
}

#[test]
fn workflow_templates_rebase_block_scalar_identity() {
    for style in ["|-", ">-"] {
        let source = yaml_source(&format!("{style}\n  {}", workflow(1)));
        // The helper fixture puts a comment after the scalar; remove it from
        // block content so that the decoded value remains a UUID.
        let source = source.replace(" # keep this comment", "");
        let result = rebase_workflow_id(&WorkflowSourceDecoder, &source, workflow(20)).unwrap();
        let parsed =
            compiler::parse_workflow(&decode(&result).unwrap(), VersionId::new(Uuid::nil()))
                .unwrap();
        assert_eq!(parsed.definition.workflow_id, workflow(20));
        assert!(result.contains("input_schema: true\n"));
        assert_eq!(
            result,
            source.replacen(
                &format!("workflow_id: {style}\n  {}", workflow(1)),
                &format!("workflow_id: {style}\n  {}", workflow(20)),
                1
            )
        );
    }
}
