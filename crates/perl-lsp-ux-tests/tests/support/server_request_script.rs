use super::write_shared_message;
use anyhow::{Context, Result, anyhow};
use serde_json::{Map, Value, json};
use std::collections::VecDeque;
use std::process::ChildStdin;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub enum ScriptedServerResponse {
    Success { result: Value, delay: Duration },
    Error { code: i64, message: String, data: Option<Value>, delay: Duration },
    NoResponse,
}

impl ScriptedServerResponse {
    pub fn success(result: Value) -> Self {
        Self::Success { result, delay: Duration::ZERO }
    }

    pub fn error_with_data(code: i64, message: impl Into<String>, data: Value) -> Self {
        Self::Error { code, message: message.into(), data: Some(data), delay: Duration::ZERO }
    }

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

#[derive(Debug, Clone, PartialEq)]
pub struct ScriptedServerRequest {
    pub method: String,
    pub response: ScriptedServerResponse,
}

impl ScriptedServerRequest {
    pub fn new(method: impl Into<String>, response: ScriptedServerResponse) -> Self {
        Self { method: method.into(), response }
    }

    pub fn success(method: impl Into<String>, result: Value) -> Self {
        Self::new(method, ScriptedServerResponse::success(result))
    }

    pub fn no_response(method: impl Into<String>) -> Self {
        Self::new(method, ScriptedServerResponse::NoResponse)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerRequestDelivery {
    Unscripted,
    IntentionallyPending,
    Scheduled,
    Sent,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObservedServerRequest {
    pub id: Value,
    pub method: String,
    pub params: Value,
    pub scripted_response: Option<Value>,
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

pub struct ServerRequestScript {
    state: Arc<Mutex<State>>,
    _dispatcher: std::thread::JoinHandle<()>,
}

#[derive(Clone)]
pub struct ServerRequestObserver {
    state: Arc<Mutex<State>>,
    response_tx: Sender<(usize, ResponsePlan)>,
}

impl ServerRequestScript {
    pub fn new(
        stdin: Arc<Mutex<ChildStdin>>,
        script: Vec<ScriptedServerRequest>,
    ) -> Result<(Self, ServerRequestObserver)> {
        let state = Arc::new(Mutex::new(State { script: script.into(), observed: Vec::new() }));
        let (response_tx, response_rx) = mpsc::channel::<(usize, ResponsePlan)>();
        let dispatch_state = state.clone();
        let _dispatcher = std::thread::Builder::new()
            .name("ux-scripted-responses".into())
            .spawn(move || {
                while let Ok((index, plan)) = response_rx.recv() {
                    let worker_stdin = stdin.clone();
                    let worker_state = dispatch_state.clone();
                    let worker = std::thread::Builder::new()
                        .name(format!("ux-scripted-response-{index}"))
                        .spawn(move || {
                            if !plan.delay.is_zero() {
                                std::thread::sleep(plan.delay);
                            }
                            let delivery = match write_shared_message(&worker_stdin, &plan.response)
                            {
                                Ok(()) => ServerRequestDelivery::Sent,
                                Err(error) => ServerRequestDelivery::Failed(format!("{error:#}")),
                            };
                            set_delivery(&worker_state, index, delivery);
                        });
                    if let Err(error) = worker {
                        set_delivery(
                            &dispatch_state,
                            index,
                            ServerRequestDelivery::Failed(format!(
                                "failed to spawn response worker: {error}"
                            )),
                        );
                    }
                }
            })
            .context("failed to spawn scripted response dispatcher")?;
        let observer = ServerRequestObserver { state: state.clone(), response_tx };
        Ok((Self { state, _dispatcher }, observer))
    }

    pub fn wait(&self, timeout: Duration) -> Result<Vec<ObservedServerRequest>> {
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
                return Err(anyhow!("failed to answer {}: {}", failure.0, failure.1));
            }
            let scheduled = observed
                .iter()
                .filter(|request| request.delivery == ServerRequestDelivery::Scheduled)
                .count();
            if remaining == 0 && scheduled == 0 {
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

    pub fn assert_no_unscripted(&self) -> Result<()> {
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
}

impl ServerRequestObserver {
    pub fn observe(&self, message: &Value) {
        let Some(id) = message.get("id").filter(|id| !id.is_null()).cloned() else {
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
        if let Some(plan) = plan
            && self.response_tx.send((index, plan)).is_err()
        {
            set_delivery(
                &self.state,
                index,
                ServerRequestDelivery::Failed("response dispatcher stopped".into()),
            );
        }
    }
}

fn set_delivery(state: &Arc<Mutex<State>>, index: usize, delivery: ServerRequestDelivery) {
    let mut state = state.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(request) = state.observed.get_mut(index) {
        request.delivery = delivery;
    }
}
