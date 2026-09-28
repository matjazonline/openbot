//! Test composition root. Concrete adapter wiring lives outside the application
//! so nested application fixtures can share real decoding without depending on
//! the adapter layer. This module is never compiled into the production library.

pub(crate) mod workflow {
    pub(crate) use crate::adapters::workflow_source::decode;
}
