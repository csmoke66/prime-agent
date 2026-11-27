//! Paused background wakes at the turn runner's gate.
use super::*;

fn queued(priority: QueuePriority, queue_key: Option<&str>, done: Option<oneshot::Sender<TurnSettle>>) -> QueuedItem {
    QueuedItem {
        priority,
        preview: None,
        message: "row".to_string(),
        custom_message: None,
        agent_message: None,
        queue_key: queue_key.map(str::to_string),
        admission_id: None,
        images: Vec::new(),
        done,
        queue_visible: false,
        policy: TurnPolicy::Injected,
        forced_batch: false,
    }
}

/// Start the runner over a core that already holds `rows` and wait until it
/// parks; returns the handles the test keeps driving.
async fn parked_runner(
    runner: TurnRunner,
    rows: Vec<QueuedItem>,
) -> (Arc<Mutex<SessionCore>>, Arc<Notify>, tokio::task::JoinHandle<()>) {
    let core = Arc::clone(&runner.core);
    let work_notify = Arc::clone(&runner.work_notify);
    {
        let mut core = core.lock().unwrap();
        core.ignored_background_wakes = 3;
        core.steering.extend(rows);
    }
    let idle = Arc::clone(&runner.idle_notify);
    let mut parked = std::pin::pin!(idle.notified());
    parked.as_mut().enable();
    let task = tokio::spawn(runner.run());
    tokio::time::timeout(std::time::Duration::from_secs(5), parked)
        .await
        .expect("the runner never parked");
    (core, work_notify, task)
}

fn scripted() -> Arc<dyn SessionEngine> {
    Arc::new(
        ScriptedEngine::from_value(&json!({ "responses": ["one", "two"] })).unwrap_or_default(),
    )
}

/// Paused background rows start no turn; a human row clears the pause and the
/// held rows ride its turn.
#[tokio::test]
async fn a_human_row_lifts_the_pause_and_carries_the_held_rows() {
    let (core, work_notify, task) = parked_runner(
        burst_runner(scripted()),
        vec![queued(QueuePriority::Background, Some(INBOX_ARRIVAL_QUEUE_KEY), None)],
    )
    .await;
    {
        let core = core.lock().unwrap();
        assert!(!core.busy, "a paused session starts no background turn");
        assert_eq!(core.steering.len(), 1, "the held row stays queued");
    }
    let (done_tx, done_rx) = oneshot::channel();
    core.lock()
        .unwrap()
        .steering
        .push_front(queued(QueuePriority::Human, None, Some(done_tx)));
    work_notify.notify_one();
    let settled = tokio::time::timeout(std::time::Duration::from_secs(5), done_rx)
        .await
        .expect("the human turn never settled")
        .expect("the waiter dropped");
    assert_eq!(settled, TurnSettle::Completed);
    {
        let core = core.lock().unwrap();
        assert_eq!(core.ignored_background_wakes, 0, "a human turn lifts the pause");
        assert!(core.steering.is_empty(), "the held row rode the human turn");
    }
    task.abort();
}

