//! A counted, scripted `GenerationProvider` for tests (D9): it records every request it is
//! given, exactly as an adapter would send it, and answers from a script. It also counts calls
//! that were dropped before they finished (an aborted connection) and the most calls in flight
//! at once per description tag, so tests can prove a lost lease cancels the real call and no
//! tenant ever has two calls in flight. It makes no network calls and exists only in test
//! builds.
use super::ports::{
    GenerationProvider, ProviderError, ProviderRequest, ProviderResponse, ProviderUsage, StopReason,
};
use crate::domain::access_level::AccessLevel;
use async_trait::async_trait;
use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

type Answer = Result<ProviderResponse, ProviderError>;

/// One scripted call.
#[derive(Debug, Clone)]
pub enum Step {
    Answer(Answer),
    /// Answer after a delay (a slow provider).
    After(Duration, Answer),
    /// Never answer: only dropping the call ends it.
    Hang,
}

pub struct CountedProvider {
    script: Mutex<Vec<Step>>,
    /// Answer every call beyond the script with this instead of failing the test.
    repeat: Option<Step>,
    requests: Mutex<Vec<ProviderRequest>>,
    /// When each call started, in call order.
    started: Mutex<Vec<tokio::time::Instant>>,
    max_level: AccessLevel,
    dropped: AtomicUsize,
    /// `dropped` as each call started, in call order (ordering evidence).
    drops_at_start: Mutex<Vec<usize>>,
    in_flight: Mutex<HashMap<String, (usize, usize)>>,
}

pub fn usage() -> ProviderUsage {
    ProviderUsage {
        input_tokens: 900,
        cache_creation_input_tokens: 0,
        cache_read_input_tokens: 100,
        output_tokens: 200,
    }
}

/// A complete answer with `text`.
pub fn answer(text: &str) -> Answer {
    Ok(ProviderResponse {
        stop: StopReason::EndTurn,
        text: text.to_owned(),
        usage: usage(),
    })
}

/// The tag a request's in-flight count is kept under: its `description` (tests put the
/// tenant there), or empty.
fn tag(request: &ProviderRequest) -> String {
    serde_json::from_str::<serde_json::Value>(&request.user)
        .ok()
        .and_then(|user| user["description"].as_str().map(str::to_owned))
        .unwrap_or_default()
}

struct Flight<'a> {
    provider: &'a CountedProvider,
    tag: String,
    done: bool,
}

impl Drop for Flight<'_> {
    fn drop(&mut self) {
        if let Ok(mut flights) = self.provider.in_flight.lock()
            && let Some((current, _)) = flights.get_mut(&self.tag)
        {
            *current -= 1;
        }
        if !self.done {
            self.provider.dropped.fetch_add(1, Ordering::SeqCst);
        }
    }
}

impl CountedProvider {
    /// Answers in order; a call beyond the script panics, so an unexpected call fails a test.
    pub fn new(script: Vec<Answer>) -> Self {
        Self::steps(script.into_iter().map(Step::Answer).collect())
    }

    pub fn steps(script: Vec<Step>) -> Self {
        Self {
            script: Mutex::new(script.into_iter().rev().collect()),
            repeat: None,
            requests: Mutex::new(Vec::new()),
            started: Mutex::new(Vec::new()),
            max_level: AccessLevel::MAX,
            dropped: AtomicUsize::new(0),
            drops_at_start: Mutex::new(Vec::new()),
            in_flight: Mutex::new(HashMap::new()),
        }
    }

    /// After the script, every call gets `step`.
    pub fn then_always(mut self, step: Step) -> Self {
        self.repeat = Some(step);
        self
    }

    pub fn with_max_level(mut self, level: AccessLevel) -> Self {
        self.max_level = level;
        self
    }

    pub fn calls(&self) -> usize {
        self.requests.lock().unwrap().len()
    }

    pub fn requests(&self) -> Vec<ProviderRequest> {
        self.requests.lock().unwrap().clone()
    }

    /// When each call started, in call order.
    pub fn started(&self) -> Vec<tokio::time::Instant> {
        self.started.lock().unwrap().clone()
    }

    /// Calls dropped before they answered (the connection would have been aborted).
    pub fn dropped(&self) -> usize {
        self.dropped.load(Ordering::SeqCst)
    }

    /// How many calls had been dropped when each call started.
    pub fn drops_at_start(&self) -> Vec<usize> {
        self.drops_at_start.lock().unwrap().clone()
    }

    /// The most calls ever in flight at once under `tag`.
    pub fn max_in_flight(&self, tag: &str) -> usize {
        self.in_flight
            .lock()
            .unwrap()
            .get(tag)
            .map_or(0, |(_, max)| *max)
    }
}

#[async_trait]
impl GenerationProvider for CountedProvider {
    fn max_level(&self) -> AccessLevel {
        self.max_level
    }

    async fn generate(&self, request: &ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        self.requests.lock().unwrap().push(request.clone());
        self.started
            .lock()
            .unwrap()
            .push(tokio::time::Instant::now());
        self.drops_at_start.lock().unwrap().push(self.dropped());
        let step = self
            .script
            .lock()
            .unwrap()
            .pop()
            .or_else(|| self.repeat.clone())
            .expect("an unscripted provider call");
        let tag = tag(request);
        {
            let mut flights = self.in_flight.lock().unwrap();
            let entry = flights.entry(tag.clone()).or_default();
            entry.0 += 1;
            entry.1 = entry.1.max(entry.0);
        }
        let mut flight = Flight {
            provider: self,
            tag,
            done: false,
        };
        let answer = match step {
            Step::Answer(answer) => answer,
            Step::After(delay, answer) => {
                tokio::time::sleep(delay).await;
                answer
            }
            Step::Hang => std::future::pending().await,
        };
        flight.done = true;
        answer
    }
}
