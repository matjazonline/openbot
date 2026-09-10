//! Same inline MiniJinja semantics/schema as the pinned tool, with fuel and an output writer.
//! No loader is installed and render_file is forbidden at the common dispatch boundary.
use crate::services::harness::ToolInvocation;
use serde_json::{Value, json};
use std::io::{self, Write};

pub(super) fn render(args: Value) -> ToolInvocation {
    if !args
        .as_object()
        .is_some_and(|args| args.contains_key("data"))
    {
        return ToolInvocation::failure("Template data is required");
    }
    let Some(template) = args.get("template").and_then(Value::as_str) else {
        return ToolInvocation::failure("Inline template is required");
    };
    let mut environment = minijinja::Environment::new();
    environment.set_trim_blocks(true);
    environment.set_lstrip_blocks(true);
    environment.set_fuel(Some(10_000));
    environment.set_recursion_limit(32);
    environment.remove_global("range");
    for filter in ["center", "indent", "wordwrap", "format", "replace"] {
        environment.remove_filter(filter);
    }
    let Ok(template) = environment.template_from_str(template) else {
        return ToolInvocation::failure("Invalid inline template");
    };
    let mut output = LimitedOutput(Vec::new());
    if template
        .render_captured_to(&args["data"], &mut output)
        .is_err()
    {
        return ToolInvocation::failure("Template failed or exceeded its execution/output budget");
    }
    match String::from_utf8(output.0) {
        Ok(rendered) => ToolInvocation::success(json!({"rendered": rendered})),
        Err(_) => ToolInvocation::failure("Invalid template output"),
    }
}

struct LimitedOutput(Vec<u8>);
impl Write for LimitedOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > 65_536 {
            return Err(io::Error::other("Template output budget exceeded"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
