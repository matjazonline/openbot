//! The standalone memory service as OpenBot consumes it (phase 05, decisions E1–E2, E6, E8).
//!
//! Ports only: the HTTP implementation is an outer adapter and speaks nothing but the
//! service's published contract. OpenBot never stores memory content itself.
mod delivery;
mod lifecycle;
mod maintenance;
mod ports;
mod reverify;
mod settings;

pub use delivery::*;
pub use lifecycle::*;
pub use maintenance::*;
pub use ports::*;
pub use reverify::*;
pub use settings::*;
