use super::*;

#[test]
fn channel_reply_terminal_sample_returns_business_value_without_a_script() {
    let value = json!({"answer":"done","no_reply":false});
    let mut source = source("channel.no_reply");
    source["steps"]["start"]["with"] =
        json!({"value":{"literal":value},"output_schema":{"literal":true}});
    let report = execute(
        build(&source, DependencySnapshots::default(), vec![]),
        request(vec![]),
    )
    .unwrap();
    complete(&report);
    assert_eq!(report.output, Some(value));
    assert!(report.effects.is_empty());
    source["output_schema"] = json!({"type":"integer"});
    let report = execute(
        build(&source, DependencySnapshots::default(), vec![]),
        request(vec![]),
    )
    .unwrap();
    assert_eq!(report.status, SampleStatus::Stopped);
    assert!(report.output.is_none());
}
