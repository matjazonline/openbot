use super::tests::*;
use super::*;
use crate::application::workflow::jev::assets::tests as images;
fn sample() -> AuthoringExample {
    let mut sample = example("jev.evaluate").unwrap();
    let inputs = images::inputs(&images::png(280, 280));
    changed(&mut sample, |source| {
        source["steps"]["start"]["with"]["assets"] = json!({"literal":inputs["assets"]});
        source["steps"]["start"]["with"]["questions"] = json!({"literal":inputs["questions"]});
    });
    sample
}
#[test]
fn jev_box_mixed_output_coordinates_confidence_and_static_paths() {
    let compiled = compile_example(&sample()).unwrap();
    compiled
        .validate_step_output(&step(), &images::output())
        .unwrap();
    for (path, value) in [
        ("/answers/mug/box/left", json!(-0.1)),
        ("/answers/mug/box/right", json!(1.1)),
        ("/answers/mug/box/bottom", json!(0.2)),
        ("/answers/mug/box/top", json!(0.95)),
        ("/answers/mug/box/left", json!(0.8)),
        ("/answers/mug/confidence", json!(1.01)),
    ] {
        let mut bad = images::output();
        *bad.pointer_mut(path).unwrap() = value;
        assert!(
            compiled.validate_step_output(&step(), &bad).is_err(),
            "{path}"
        );
    }
    for (path, valid) in [
        ("/answers/mug/box/left", true),
        ("/answers/mug/confidence", true),
        ("/answers/visible/noul", true),
        ("/answers/mug/box/x", false),
        ("/answers/mug/probabilities", false),
    ] {
        let mut sample = sample();
        changed(&mut sample, |source| {
            source["steps"]["start"]["routes"]["success"] = json!("next");
            source["steps"]["next"] = json!({"type":"data.map","with":{"value":{"ref":format!("/steps/start/output{path}")},"output_schema":{"literal":{"type":"number"}}},"routes":{"success":"$end"}});
        });
        assert_eq!(compile_example(&sample).is_ok(), valid, "{path}");
    }
}
#[test]
fn jev_box_requires_declared_asset_version_and_no_criteria() {
    for (field, value) in [
        ("criteria", json!([])),
        ("criteria", json!(null)),
        ("instructions", json!("Find mug")),
        ("instructions", json!("Find mug in `assets.missing`")),
    ] {
        let mut sample = sample();
        changed(&mut sample, |source| {
            source["steps"]["start"]["with"]["questions"]["literal"]["mug"][field] = value
        });
        assert!(compile_example(&sample).is_err());
    }
    for model in ["jev-1.12.0", "jev-1.13.0", "jev-1.15.0"] {
        let mut sample = sample();
        changed(&mut sample, |source| {
            source["steps"]["start"]["with"]["model"] = json!({"literal":model})
        });
        assert!(compile_example(&sample).is_err());
    }
    let mut sample = sample();
    changed(&mut sample, |source| {
        source["steps"]["start"]["with"]["assets"] = json!({"ref":"/input"})
    });
    assert!(compile_example(&sample).is_err());
}
