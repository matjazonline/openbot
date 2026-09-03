//! Protocol adapters: they parse what a provider sent, render what it will accept, and make the
//! provider call.
//!
//! Nothing here declares an abstraction the application consumes. The transport ports live in
//! [`crate::transport`], with the workers and use cases that drive them; an adapter's job is to
//! implement them. The `ProtocolEgressAdapter`/`EgressRegistry` pair that used to live in this
//! module was an abstraction inside the layer it abstracted -- and its one implementation had to
//! invent a channel name and a company slug because the normalized message it was handed carried
//! neither. `DeliveryEnvelope` exists so a renderer is given a resolved destination instead.

pub mod email;
