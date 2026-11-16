//! Added native-to-workflow authority checks must not introduce blocking owner edges.
#[derive(Clone, Copy)]
pub(in crate::adapters::persistence) enum RowLocks {
    Wait,
    NoWait,
}

impl RowLocks {
    /// Statements passed here end in their row-lock clause. The mode is application
    /// control flow, never a configuration value or deserialized native input.
    pub(in crate::adapters::persistence) fn sql(self, statement: &str) -> String {
        match self {
            Self::Wait => statement.to_owned(),
            Self::NoWait => format!("{statement} NOWAIT"),
        }
    }
}
