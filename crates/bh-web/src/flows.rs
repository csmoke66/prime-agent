//! Browser-driven credential flows: a registry of running flows, the event log each one streams to
//! the page, and the REST handlers that start, poll, answer, and cancel them.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{oneshot, watch, Notify};

use crate::host::{FlowOutcome, FlowRequest, FlowUi, PromptInput, WebHost};
use crate::HostState;

/// How long one poll waits for news before answering with no events.
const POLL_WAIT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub(crate) enum FlowEvent {
    Progress {
        message: String,
    },
    Waiting {
        message: String,
    },
    AuthUrl {
        url: String,
        instructions: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Prompt {
        prompt: String,
        masked: bool,
        allow_empty: bool,
    },
    Done {
        outcome: FlowOutcome,
    },
}

struct FlowState {
    events: Vec<FlowEvent>,
    answer: Option<oneshot::Sender<Option<String>>>,
}

/// One running flow: its event log, the pending prompt, and the cancel signal.
pub(crate) struct FlowShared {
    state: Mutex<FlowState>,
    changed: Notify,
    cancel: watch::Sender<bool>,
}

impl FlowShared {
    fn new() -> Self {
        FlowShared {
            state: Mutex::new(FlowState {
                events: Vec::new(),
                answer: None,
            }),
            changed: Notify::new(),
            cancel: watch::channel(false).0,
        }
    }

    fn lock(&self) -> MutexGuard<'_, FlowState> {
        self.state.lock().expect("flow state lock")
    }

    fn push(&self, event: FlowEvent) {
        self.lock().events.push(event);
        self.changed.notify_waiters();
    }

    pub(crate) fn progress(&self, message: String) {
        self.push(FlowEvent::Progress { message });
    }

    pub(crate) fn waiting(&self, message: String) {
        self.push(FlowEvent::Waiting { message });
    }

    pub(crate) fn auth_url(&self, url: &str, instructions: Option<&str>) {
        self.push(FlowEvent::AuthUrl {
            url: url.to_string(),
            instructions: instructions.map(str::to_string),
        });
    }

    pub(crate) async fn prompt(&self, prompt: &str, input: PromptInput) -> Option<String> {
        if self.cancelled() {
            return None;
        }
        let (reply, answer) = oneshot::channel();
        {
            let mut state = self.lock();
            state.answer = Some(reply);
            state.events.push(FlowEvent::Prompt {
                prompt: prompt.to_string(),
                masked: input.masked,
                allow_empty: input.allow_empty,
            });
        }
        self.changed.notify_waiters();
        tokio::select! {
            answered = answer => answered.unwrap_or(None),
            () = self.cancelled_wait() => None,
        }
    }

    pub(crate) fn cancelled(&self) -> bool {
        *self.cancel.borrow()
    }

    pub(crate) async fn cancelled_wait(&self) {
        let mut cancelled = self.cancel.subscribe();
        // `wait_for` errors only when the sender is dropped, which cannot happen while `self` lives.
        let _ = cancelled.wait_for(|cancelled| *cancelled).await;
    }

    fn is_done(&self) -> bool {
        matches!(self.lock().events.last(), Some(FlowEvent::Done { .. }))
    }

    fn cancel(&self) {
        self.cancel.send_replace(true);
        self.lock().answer = None;
    }
}

/// The running and recently finished flows of this server.
#[derive(Default)]
pub(crate) struct Flows {
    running: Mutex<HashMap<u64, Arc<FlowShared>>>,
    next_id: AtomicU64,
}

impl Flows {
    fn get(&self, id: u64) -> Result<Arc<FlowShared>, (StatusCode, String)> {
        self.running
            .lock()
            .expect("flows lock")
            .get(&id)
            .cloned()
            .ok_or((StatusCode::NOT_FOUND, "no such flow".to_string()))
    }
}

pub(crate) async fn start<H: WebHost>(
    Extension(host): Extension<Arc<HostState<H>>>,
    Json(request): Json<FlowRequest>,
) -> Json<Value> {
    let shared = Arc::new(FlowShared::new());
    let id = host.flows.next_id.fetch_add(1, Ordering::Relaxed);
    {
        let mut running = host.flows.running.lock().expect("flows lock");
        running.retain(|_, flow| !flow.is_done());
        running.insert(id, Arc::clone(&shared));
    }
    let ui = FlowUi {
        shared: Arc::clone(&shared),
    };
    tokio::spawn(async move {
        let outcome = host.host.run_flow(request, ui).await;
        shared.push(FlowEvent::Done { outcome });
    });
    Json(json!({ "flowId": id }))
}

#[derive(Deserialize)]
pub(crate) struct PollQuery {
    #[serde(default)]
    since: usize,
}

/// Events from index `since` on; waits for news when there is none yet.
pub(crate) async fn poll<H: WebHost>(
    Extension(host): Extension<Arc<HostState<H>>>,
    Path(id): Path<u64>,
    Query(query): Query<PollQuery>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let flow = host.flows.get(id)?;
    let read = || {
        let state = flow.lock();
        (state.events.get(query.since..).unwrap_or_default().to_vec(), state.events.len())
    };
    let mut news = std::pin::pin!(flow.changed.notified());
    news.as_mut().enable();
    let (mut events, mut next) = read();
    if events.is_empty() {
        let _ = tokio::time::timeout(POLL_WAIT, news).await;
        (events, next) = read();
    }
    Ok(Json(json!({ "events": events, "next": next })))
}

#[derive(Deserialize)]
pub(crate) struct Answer {
    value: Option<String>,
}

pub(crate) async fn answer<H: WebHost>(
    Extension(host): Extension<Arc<HostState<H>>>,
    Path(id): Path<u64>,
    Json(answer): Json<Answer>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let flow = host.flows.get(id)?;
    let reply = flow.lock().answer.take();
    let reply = reply.ok_or((StatusCode::CONFLICT, "the flow is not waiting for input".to_string()))?;
    // A dropped receiver means the flow already ended; the answer is moot.
    let _ = reply.send(answer.value);
    Ok(Json(json!({ "ok": true })))
}

pub(crate) async fn cancel<H: WebHost>(
    Extension(host): Extension<Arc<HostState<H>>>,
    Path(id): Path<u64>,
) -> Result<Json<Value>, (StatusCode, String)> {
    host.flows.get(id)?.cancel();
    Ok(Json(json!({ "ok": true })))
}
