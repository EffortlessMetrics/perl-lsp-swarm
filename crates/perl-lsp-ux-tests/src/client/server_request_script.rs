//! Scripted handling for server-initiated JSON-RPC requests.

use anyhow::{Context, Result, anyhow};
use serde_json::{Map, Value, json};
use std::collections::VecDeque;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A response that the scripted client should send to a server request.
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptedServerResponse {
    /// Return a successful JSON-RPC result.
    Success {
        /// The JSON-RPC result value.
        result: Value,
        /// The delay before writing the response.
        delay: Duration,
    },
    /// Return a JSON-RPC error, optionally including error data.
    Error {
        /// The JSON-RPC error code.
        code: i64,
        /// The JSON-RPC error message.
        message: String,
        /// Optional JSON-RPC error data.
        data: Option<Value>,
        /// The delay before writing the response.
        delay: Duration,
    },
    /// Deliberately leave the request unanswered.
    NoResponse,
}

impl ScriptedServerResponse {
    /// Create an immediate successful response.
    pub fn success(result: Value) -> Self {
        Self::Success { result, delay: Duration::ZERO }
    }

    /// Create an immediate error response with data.
    pub fn error_with_data(code: i64, message: impl Into<String>, data: Value) -> Self {
        Self::Error { code, message: message.into(), data: Some(data), delay: Duration::ZERO }
    }

    /// Delay this response by the specified duration.
    #[must_use]
    pub fn after(mut self, delay: Duration) -> Self {
        match &mut self {
            Self::Success { delay: current, .. } | Self::Error { delay: current, .. } => {
                *current = delay;
            }
            Self::NoResponse => {}
        }
        self
    }

    fn plan(self, id: Value) -> Option<ResponsePlan> {
        match self {
            Self::Success { result, delay } => Some(ResponsePlan {
                response: json!({"jsonrpc": "2.0", "id": id, "result": result}),
                delay,
            }),
            Self::Error { code, message, data, delay } => {
                let mut error = Map::new();
                error.insert("code".into(), json!(code));
                error.insert("message".into(), Value::String(message));
                if let Some(data) = data {
                    error.insert("data".into(), data);
                }
                Some(ResponsePlan {
                    response: json!({"jsonrpc": "2.0", "id": id, "error": error}),
                    delay,
                })
            }
            Self::NoResponse => None,
        }
    }
}

/// A server-initiated request and the response scripted for its method.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptedServerRequest {
    /// The JSON-RPC method to match.
    pub method: String,
    /// The response behavior for matching requests.
    pub response: ScriptedServerResponse,
}

impl ScriptedServerRequest {
    /// Create a scripted request entry.
    pub fn new(method: impl Into<String>, response: ScriptedServerResponse) -> Self {
        Self { method: method.into(), response }
    }

    /// Create an immediate successful response entry.
    pub fn success(method: impl Into<String>, result: Value) -> Self {
        Self::new(method, ScriptedServerResponse::success(result))
    }

    /// Create an intentionally unanswered request entry.
    pub fn no_response(method: impl Into<String>) -> Self {
        Self::new(method, ScriptedServerResponse::NoResponse)
    }
}

/// The observed delivery state of a scripted server request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerRequestDelivery {
    /// No matching script entry was found.
    Unscripted,
    /// The script intentionally omitted a response.
    IntentionallyPending,
    /// A response worker has been scheduled but has not finished.
    Scheduled,
    /// The response was written to the client process.
    Sent,
    /// The response could not be written.
    Failed(String),
}

/// A server-initiated request observed by the client.
#[derive(Debug, Clone, PartialEq)]
pub struct ObservedServerRequest {
    /// The exact JSON-RPC request ID.
    pub id: Value,
    /// The request method.
    pub method: String,
    /// The request parameters, or null when omitted.
    pub params: Value,
    /// The response JSON that was scheduled, if any.
    pub scripted_response: Option<Value>,
    /// The response delivery state.
    pub delivery: ServerRequestDelivery,
}

#[derive(Debug)]
struct ResponsePlan {
    response: Value,
    delay: Duration,
}

#[derive(Debug, Default)]
struct State {
    script: VecDeque<ScriptedServerRequest>,
    observed: Vec<ObservedServerRequest>,
}

pub(crate) struct ServerRequestScript {
    state: Arc<Mutex<State>>,
    response_tx: Arc<Mutex<Option<Sender<(usize, ResponsePlan)>>>>,
    dispatcher: Option<std::thread::JoinHandle<()>>,
    workers: Arc<Mutex<Vec<std::thread::JoinHandle<()>>>>,
}

#[derive(Clone)]
pub(crate) struct ServerRequestObserver {
    state: Arc<Mutex<State>>,
    response_tx: Arc<Mutex<Option<Sender<(usize, ResponsePlan)>>>>,
}

impl ServerRequestScript {
    pub(crate) fn new(
        stdin: Arc<Mutex<Option<std::process::ChildStdin>>>,
        script: Vec<ScriptedServerRequest>,
    ) -> Result<(Self, ServerRequestObserver)> {
        let state = Arc::new(Mutex::new(State { script: script.into(), observed: Vec::new() }));
        let (response_tx, response_rx) = mpsc::channel::<(usize, ResponsePlan)>();
        let response_tx = Arc::new(Mutex::new(Some(response_tx)));
        let workers = Arc::new(Mutex::new(Vec::new()));
        let dispatch_state = state.clone();
        let dispatch_stdin = stdin.clone();
        let dispatch_workers = workers.clone();
        let dispatcher = std::thread::Builder::new()
            .name("ux-scripted-responses".into())
            .spawn(move || {
                while let Ok((index, plan)) = response_rx.recv() {
                    let worker_stdin = dispatch_stdin.clone();
                    let worker_state = dispatch_state.clone();
                    let worker = std::thread::Builder::new()
                        .name(format!("ux-scripted-response-{index}"))
                        .spawn(move || {
                            if !plan.delay.is_zero() {
                                std::thread::sleep(plan.delay);
                            }
                            let delivery = match worker_stdin.lock() {
                                Ok(mut stdin) => match stdin.as_mut() {
                                    Some(stdin) => match super::write_framed(stdin, &plan.response)
                                    {
                                        Ok(()) => ServerRequestDelivery::Sent,
                                        Err(error) => {
                                            ServerRequestDelivery::Failed(format!("{error:#}"))
                                        }
                                    },
                                    None => ServerRequestDelivery::Failed(
                                        "LSP client stdin is already closed".into(),
                                    ),
                                },
                                Err(error) => ServerRequestDelivery::Failed(format!(
                                    "failed to lock LSP client stdin: {error}"
                                )),
                            };
                            set_delivery(&worker_state, index, delivery);
                        });
                    match worker {
                        Ok(worker) => {
                            dispatch_workers
                                .lock()
                                .unwrap_or_else(|error| error.into_inner())
                                .push(worker);
                        }
                        Err(error) => set_delivery(
                            &dispatch_state,
                            index,
                            ServerRequestDelivery::Failed(format!(
                                "failed to spawn response worker: {error}"
                            )),
                        ),
                    }
                }
            })
            .context("failed to spawn scripted response dispatcher")?;
        let observer =
            ServerRequestObserver { state: state.clone(), response_tx: response_tx.clone() };
        Ok((Self { state, response_tx, dispatcher: Some(dispatcher), workers }, observer))
    }

    pub(crate) fn wait(&self, timeout: Duration) -> Result<Vec<ObservedServerRequest>> {
        let deadline = Instant::now() + timeout;
        loop {
            let (remaining, observed) = {
                let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
                (state.script.len(), state.observed.clone())
            };
            if let Some(failure) = observed.iter().find_map(|request| match &request.delivery {
                ServerRequestDelivery::Failed(reason) => {
                    Some((request.method.clone(), reason.clone()))
                }
                _ => None,
            }) {
                self.join_workers()?;
                return Err(anyhow!("failed to answer {}: {}", failure.0, failure.1));
            }
            let scheduled = observed
                .iter()
                .filter(|request| request.delivery == ServerRequestDelivery::Scheduled)
                .count();
            if remaining == 0 && scheduled == 0 {
                self.join_workers()?;
                return Ok(observed);
            }
            if Instant::now() >= deadline {
                return Err(anyhow!(
                    "timed out after {}ms waiting for scripted requests; remaining={remaining}, scheduled={scheduled}",
                    timeout.as_millis()
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub(crate) fn assert_no_unscripted(&self) -> Result<()> {
        let unscripted = self
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .observed
            .iter()
            .filter(|request| request.delivery == ServerRequestDelivery::Unscripted)
            .map(|request| format!("{} id={}", request.method, request.id))
            .collect::<Vec<_>>();
        if unscripted.is_empty() {
            Ok(())
        } else {
            Err(anyhow!("unscripted server requests: {}", unscripted.join(", ")))
        }
    }

    fn join_workers(&self) -> Result<()> {
        let workers = self
            .workers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .drain(..)
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().map_err(|_| anyhow!("scripted response worker panicked"))?;
        }
        Ok(())
    }

    pub(crate) fn settle(mut self) {
        self.response_tx.lock().unwrap_or_else(|error| error.into_inner()).take();
        if let Some(dispatcher) = self.dispatcher.take() {
            let _ = dispatcher.join();
        }
        let _ = self.join_workers();
    }
}

impl ServerRequestObserver {
    pub(crate) fn observe(&self, message: &Value) {
        let Some(id) = message.get("id").filter(|id| id.is_number() || id.is_string()).cloned()
        else {
            return;
        };
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            return;
        };
        if message.get("result").is_some() || message.get("error").is_some() {
            return;
        }
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let (index, plan) = {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            let scripted = state
                .script
                .iter()
                .position(|request| request.method == method)
                .and_then(|index| state.script.remove(index));
            let (scripted_response, delivery, plan) = match scripted {
                Some(request) => match request.response.plan(id.clone()) {
                    Some(plan) => {
                        (Some(plan.response.clone()), ServerRequestDelivery::Scheduled, Some(plan))
                    }
                    None => (None, ServerRequestDelivery::IntentionallyPending, None),
                },
                None => (None, ServerRequestDelivery::Unscripted, None),
            };
            let index = state.observed.len();
            state.observed.push(ObservedServerRequest {
                id,
                method: method.to_string(),
                params,
                scripted_response,
                delivery,
            });
            (index, plan)
        };
        if let Some(plan) = plan {
            let send_result = self
                .response_tx
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_ref()
                .map(|sender| sender.send((index, plan)));
            if send_result.is_none_or(|result| result.is_err()) {
                set_delivery(
                    &self.state,
                    index,
                    ServerRequestDelivery::Failed("response dispatcher stopped".into()),
                );
            }
        }
    }
}

fn set_delivery(state: &Arc<Mutex<State>>, index: usize, delivery: ServerRequestDelivery) {
    let mut state = state.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(request) = state.observed.get_mut(index) {
        request.delivery = delivery;
    }
}
