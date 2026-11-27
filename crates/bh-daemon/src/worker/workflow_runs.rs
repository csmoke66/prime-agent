use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use serde_json::{json, Value};
use uuid::Uuid;

#[derive(Default)]
pub(crate) struct WorkflowRunRegistry {
    state: Mutex<RegistryState>,
}

#[derive(Default)]
struct RegistryState {
    by_key: HashMap<String, String>,
    finished: VecDeque<String>,
    runs: HashMap<String, WorkflowRun>,
}

struct WorkflowRun {
    key: String,
    name: String,
    state: &'static str,
    output: Option<Value>,
    error: Option<String>,
}

impl WorkflowRunRegistry {
    pub(crate) fn begin(&self, generation: &str, name: &str, input: &Value) -> (String, bool) {
        let key = format!(
            "{generation}\0{name}\0{}",
            serde_json::to_string(input).unwrap_or_default()
        );
        let mut state = self.state.lock().unwrap();
        if let Some(id) = state.by_key.get(&key) {
            return (id.clone(), true);
        }
        let id = Uuid::new_v4().simple().to_string();
        state.by_key.insert(key.clone(), id.clone());
        state.runs.insert(
            id.clone(),
            WorkflowRun {
                key,
                name: name.to_owned(),
                state: "queued",
                output: None,
                error: None,
            },
        );
        (id, false)
    }

    pub(crate) fn mark_running(&self, id: &str) {
        if let Some(run) = self.state.lock().unwrap().runs.get_mut(id) {
            run.state = "running";
        }
    }

    pub(crate) fn finish(&self, id: &str, result: &Result<Value, anyhow::Error>) {
        let mut state = self.state.lock().unwrap();
        let Some(run) = state.runs.get_mut(id) else {
            return;
        };
        run.state = if result.is_ok() {
            "completed"
        } else {
            "failed"
        };
        match result {
            Ok(output) => run.output = Some(output.clone()),
            Err(error) => run.error = Some(format!("{error:#}")),
        }
        let key = run.key.clone();
        state.by_key.remove(&key);
        state.finished.push_back(id.to_string());
        while state.finished.len() > 128 {
            if let Some(old) = state.finished.pop_front() {
                state.runs.remove(&old);
            }
        }
    }

    pub(crate) fn fail(&self, id: &str, error: impl Into<String>) {
        let mut state = self.state.lock().unwrap();
        let Some(run) = state.runs.get_mut(id) else {
            return;
        };
        run.state = "failed";
        run.error = Some(error.into());
        let key = run.key.clone();
        state.by_key.remove(&key);
        state.finished.push_back(id.to_string());
        while state.finished.len() > 128 {
            if let Some(old) = state.finished.pop_front() {
                state.runs.remove(&old);
            }
        }
    }

    pub(crate) fn status(&self, id: &str) -> Option<Value> {
        let state = self.state.lock().unwrap();
        let run = state.runs.get(id)?;
        let mut value = json!({"runId": id, "workflow": run.name, "state": run.state});
        if let Some(output) = &run.output {
            value["output"] = output.clone();
        }
        if let Some(error) = &run.error {
            value["error"] = json!(error);
        }
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_active_runs_coalesce_but_completed_runs_can_repeat() {
        let runs = WorkflowRunRegistry::default();
        let input = json!({"scope":"local"});
        let (first, duplicate) = runs.begin("g1", "repair", &input);
        assert!(!duplicate);
        let (second, duplicate) = runs.begin("g1", "repair", &input);
        assert!(duplicate);
        assert_eq!(first, second);
        runs.mark_running(&first);
        assert_eq!(runs.status(&first).unwrap()["state"], "running");
        runs.finish(&first, &Ok(json!({"fixed":true})));
        assert_eq!(runs.status(&first).unwrap()["output"]["fixed"], true);
        let (third, duplicate) = runs.begin("g1", "repair", &input);
        assert!(!duplicate);
        assert_ne!(first, third);
    }
}
