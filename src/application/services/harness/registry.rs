//! Which harnesses this deployment can actually run, keyed by kind.
//!
//! Lives in the application/service layer rather than under `adapters` because dispatch is what
//! consults it; the adapters are what register themselves into it. [`TransportRegistry`] carries
//! the same note for the same reason, and `src/AGENTS.md` states the rule directly: *"an
//! abstraction must not live inside the outer adapter it is intended to abstract"*.
//!
//! [`TransportRegistry`]: crate::transport::TransportRegistry

use std::{collections::HashMap, sync::Arc};

use crate::entities::harness::HarnessKind;

use super::ports::AgentHarness;

/// A harness kind with no registered implementation.
///
/// A configuration fact about this deployment, not an internal fault, so it is reported as the
/// unsupported harness it is.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("no harness is registered for '{0}'")]
pub struct UnsupportedHarness(HarnessKind);

impl UnsupportedHarness {
    pub fn kind(&self) -> HarnessKind {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HarnessRegistrationError {
    #[error("{0} is already registered")]
    Duplicate(HarnessKind),
    #[error("a {registered} harness cannot be registered as {declared}")]
    Mismatched {
        declared: HarnessKind,
        registered: HarnessKind,
    },
}

#[derive(Clone, Default)]
pub struct HarnessRegistry {
    harnesses: HashMap<HarnessKind, Arc<dyn AgentHarness>>,
}

impl HarnessRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `harness` into the `declared` slot.
    ///
    /// Consuming and fallible, copying [`TransportRegistry::register`] rather than
    /// `MemoryProviderRegistry::register`. The latter is infallible and last-write-wins, which is
    /// fine for two providers wired conditionally from config and wrong here: this sits on the
    /// dispatch path, where a silently-replaced harness is a silently-changed agent.
    ///
    /// The slot is named at the call site rather than taken from `harness.kind()`, so boot wiring
    /// says which harness it believes it is installing and a constructor returning the wrong one
    /// fails here instead of running every agent of that kind on the wrong runtime.
    ///
    /// [`TransportRegistry::register`]: crate::transport::TransportRegistry::register
    pub fn register(
        mut self,
        declared: HarnessKind,
        harness: Arc<dyn AgentHarness>,
    ) -> Result<Self, HarnessRegistrationError> {
        let registered = harness.kind();
        if registered != declared {
            return Err(HarnessRegistrationError::Mismatched {
                declared,
                registered,
            });
        }
        if self.harnesses.contains_key(&declared) {
            return Err(HarnessRegistrationError::Duplicate(declared));
        }
        self.harnesses.insert(declared, harness);
        Ok(self)
    }

    pub fn get(&self, kind: HarnessKind) -> Option<&Arc<dyn AgentHarness>> {
        self.harnesses.get(&kind)
    }

    /// The harness for `kind`, or the error the run should fail with.
    ///
    /// A typed error rather than an `Option` the caller may unwrap into a default: an agent whose
    /// harness has no implementation must fail loudly. Falling back to another harness would mean
    /// a deployment that drops one silently downgrades every agent using it -- the same agent,
    /// same prompt, different tools and a different sandbox, with nothing in the log to say so.
    pub fn require(&self, kind: HarnessKind) -> Result<&Arc<dyn AgentHarness>, UnsupportedHarness> {
        self.get(kind).ok_or(UnsupportedHarness(kind))
    }

    /// Every registered kind, in a stable order so a boot log line reads the same every time.
    pub fn registered(&self) -> Vec<HarnessKind> {
        let mut kinds: Vec<HarnessKind> = self.harnesses.keys().copied().collect();
        kinds.sort_unstable_by_key(|kind| kind.as_str());
        kinds
    }
}
