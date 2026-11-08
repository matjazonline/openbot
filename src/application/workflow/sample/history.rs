//! Bounded scripted continuity owned only by one evaluator and generated frame.
use super::*;
use crate::application::agent::AgentKey;
use crate::application::services::harness::runs::{
    LogicalSeed, LogicalSeedPair, MAX_LOGICAL_SEED_BYTES, MAX_LOGICAL_SEED_PAIRS,
};
use crate::entities::agent_capability::AgentId;

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Stream {
    frame: RunId,
    agent: AgentId,
}

#[derive(Default)]
struct History {
    recent: VecDeque<LogicalSeedPair>,
    completed: usize,
}

#[derive(Default)]
pub(super) struct SampleHistoryStore(BTreeMap<Stream, History>);

pub(super) struct Capture {
    stream: Stream,
    input: Value,
    report: SampleAgentHistory,
}

impl SampleHistoryStore {
    pub(super) fn prepare(
        &self,
        bundle: &PublishedBundle,
        frame: &Frame,
        activation: &ActivatedExecution,
    ) -> AppResult<Option<Capture>> {
        let kind = bundle.compiled().graph().definition().steps[&activation.step]
            .step_type
            .as_str();
        if kind != "agent.run" {
            return Ok(None);
        }
        let key = activation
            .inputs
            .get("agent")
            .and_then(Value::as_str)
            .and_then(AgentKey::parse)
            .ok_or_else(|| refused("Invalid sample agent"))?;
        let agent = bundle.agent_reference(bundle.company_id(), &key)?.agent_id;
        let stream = Stream {
            frame: frame.metadata.run_id,
            agent,
        };
        let history = self.0.get(&stream);
        let seed = history.map(History::seed).transpose()?.unwrap_or_default();
        let omitted_pairs = history.map_or(0, |h| h.completed - seed.pairs().len());
        Ok(Some(Capture {
            stream,
            input: activation
                .inputs
                .get("context")
                .cloned()
                .ok_or_else(|| refused("Missing sample agent context"))?,
            report: SampleAgentHistory {
                agent,
                seed,
                omitted_pairs,
            },
        }))
    }

    pub(super) fn complete(
        &mut self,
        capture: Option<Capture>,
        output: &Value,
    ) -> AppResult<Option<SampleAgentHistory>> {
        let Some(capture) = capture else {
            return Ok(None);
        };
        let history = self.0.entry(capture.stream).or_default();
        history.completed = history
            .completed
            .checked_add(1)
            .ok_or_else(|| refused("Sample history count overflow"))?;
        history.recent.push_back(LogicalSeedPair {
            input: capture.input,
            output: output.clone(),
        });
        if history.recent.len() > MAX_LOGICAL_SEED_PAIRS {
            history.recent.pop_front();
        }
        Ok(Some(capture.report))
    }
}

impl History {
    fn seed(&self) -> AppResult<LogicalSeed> {
        let mut bytes = 2usize;
        let mut pairs = Vec::new();
        for pair in self.recent.iter().rev() {
            let length = serde_json::to_vec(pair)
                .map_err(|_| refused("Invalid sample history pair"))?
                .len();
            let next = bytes
                .saturating_add(length)
                .saturating_add(usize::from(!pairs.is_empty()));
            if next > MAX_LOGICAL_SEED_BYTES {
                break;
            }
            bytes = next;
            pairs.push(pair.clone());
        }
        pairs.reverse();
        LogicalSeed::new(pairs)
    }
}
