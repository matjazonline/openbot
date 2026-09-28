use super::{Diagnostic, SourceSpan};
use serde::Serialize;
use serde_json::Value;
use std::io::{self, Write};

pub(crate) const MAX_COMPILED_FACT_BYTES: usize = 1_048_576;

/// Shared by borrowed preflight and exact expanded representation accounting.
/// Never buffers serialized bytes or clones caller-owned schemas.
pub(crate) struct FactBudget {
    remaining: usize,
    nodes: usize,
    span: SourceSpan,
}
impl FactBudget {
    pub(crate) fn new(span: SourceSpan) -> Self {
        Self {
            remaining: MAX_COMPILED_FACT_BYTES,
            nodes: 65_536,
            span,
        }
    }
    pub(crate) fn charge(&mut self, value: &impl Serialize) -> Result<(), Diagnostic> {
        serde_json::to_writer(&mut *self, value).map_err(|_| self.error())
    }
    pub(crate) fn schema(&mut self, value: &Value) -> Result<(), Diagnostic> {
        self.visit(value, 0)?;
        self.charge(value)
    }
    fn visit(&mut self, value: &Value, depth: usize) -> Result<(), Diagnostic> {
        if depth > 64 || self.nodes == 0 {
            return Err(self.error());
        }
        self.nodes -= 1;
        match value {
            Value::Object(map) => {
                for child in map.values() {
                    self.visit(child, depth + 1)?;
                }
            }
            Value::Array(values) => {
                for child in values {
                    self.visit(child, depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    pub(crate) fn punctuation(&mut self, bytes: usize) -> Result<(), Diagnostic> {
        self.remaining = self
            .remaining
            .checked_sub(bytes)
            .ok_or_else(|| self.error())?;
        Ok(())
    }
    fn error(&self) -> Diagnostic {
        Diagnostic::at(
            "dependency.limit",
            "Expanded descriptor and dependency facts exceed byte or work limits",
            "/steps",
            self.span,
        )
    }
}
impl Write for FactBudget {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.remaining = self
            .remaining
            .checked_sub(bytes.len())
            .ok_or_else(|| io::Error::other("compiled facts limit"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
