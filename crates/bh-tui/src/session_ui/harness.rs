//! The client-local harness browser. Storage jobs use the attached session file,
//! and only the job owned by the current browser can update its rows.

use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::Value;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use super::{AgentView, SessionUi};
use crate::harness_browser::{HarnessAction, HarnessBrowser};

const REFRESH_INTERVAL: Duration = Duration::from_secs(2);

pub(crate) enum HarnessWork {
    Closed,
    Idle(Instant),
    Reading(JoinHandle<Result<Vec<Value>>>),
    Deleting(JoinHandle<Result<Value>>),
}

pub(crate) enum HarnessUpdate {
    Refresh,
    Entries(Result<Vec<Value>>),
    Deleted(Result<Value>),
}

impl HarnessWork {
    pub(crate) async fn next(&mut self) -> HarnessUpdate {
        match self {
            Self::Closed => std::future::pending().await,
            Self::Idle(at) => {
                tokio::time::sleep_until(*at).await;
                HarnessUpdate::Refresh
            }
            Self::Reading(job) => HarnessUpdate::Entries(
                job.await
                    .map_err(anyhow::Error::from)
                    .and_then(|result| result),
            ),
            Self::Deleting(job) => HarnessUpdate::Deleted(
                job.await
                    .map_err(anyhow::Error::from)
                    .and_then(|result| result),
            ),
        }
    }

    pub(super) fn retire(&mut self) {
        if let Self::Reading(job) = self {
            job.abort();
        }
        // A confirmed write can finish against its original file; its result cannot be
        // consumed by a reopened browser or a different attachment.
        *self = Self::Closed;
    }

    pub(crate) fn has_pending_job(&self) -> bool {
        matches!(self, Self::Reading(_) | Self::Deleting(_))
    }
}

impl SessionUi {
    pub(super) fn open_harness_browser(&mut self, view: &mut AgentView) {
        if !self.start_harness_read() {
            self.error_row("Harness storage is unavailable in this client", view);
            return;
        }
        view.harness_browser = Some(HarnessBrowser::loading());
        self.track_menu_opened("harness", "command");
        self.dirty = true;
    }

    fn start_harness_read(&mut self) -> bool {
        let Some(settings) = self.client_settings.clone() else {
            return false;
        };
        let session_file = self.session_file.clone();
        self.harness_work = HarnessWork::Reading(tokio::task::spawn_blocking(move || {
            settings.harness_entries(session_file.as_deref())
        }));
        true
    }

    pub(crate) fn dismiss_retired_harness_browser(&mut self, view: &mut AgentView) {
        if matches!(self.harness_work, HarnessWork::Closed) && view.harness_browser.take().is_some() {
            self.dirty = true;
        }
    }

    pub(super) fn close_harness_browser(&mut self, view: &mut AgentView) {
        self.harness_work.retire();
        self.dismiss_retired_harness_browser(view);
    }

    pub(super) fn handle_harness_key(&mut self, key: &str, view: &mut AgentView) {
        self.dismiss_retired_harness_browser(view);
        let Some(browser) = view.harness_browser.as_mut() else {
            return;
        };
        match browser.handle_key(key) {
            HarnessAction::None => {}
            HarnessAction::Close => {
                self.close_harness_browser(view);
                return;
            }
            HarnessAction::Refresh => {
                if matches!(self.harness_work, HarnessWork::Idle(_)) {
                    self.start_harness_read();
                }
            }
            HarnessAction::Delete(entry) => {
                let Some(settings) = self.client_settings.clone() else {
                    browser.set_error("Harness storage is unavailable in this client");
                    self.dirty = true;
                    return;
                };
                // A poll issued before confirmation cannot repaint a pre-delete snapshot.
                // The storage layer also checks that the exact selected row is still current.
                self.harness_work.retire();
                let session_file = self.session_file.clone();
                self.harness_work = HarnessWork::Deleting(tokio::task::spawn_blocking(move || {
                    settings.delete_harness_entry(session_file.as_deref(), &entry)?;
                    Ok(entry)
                }));
            }
        }
        self.dirty = true;
    }

    pub(crate) fn apply_harness_update(&mut self, update: HarnessUpdate, view: &mut AgentView) {
        self.dismiss_retired_harness_browser(view);
        let Some(browser) = view.harness_browser.as_mut() else {
            return;
        };
        match update {
            HarnessUpdate::Refresh => {
                self.start_harness_read();
                return;
            }
            HarnessUpdate::Entries(result) => {
                self.dirty |=
                    browser.replace_entries(result.context("Could not load harness entries"));
            }
            HarnessUpdate::Deleted(Ok(entry)) => {
                browser.remove_entry(&entry);
                self.start_harness_read();
                self.dirty = true;
                return;
            }
            HarnessUpdate::Deleted(Err(error)) => {
                browser.set_error(format!("Could not delete harness entry: {error:#}"));
                self.dirty = true;
            }
        }
        self.harness_work = HarnessWork::Idle(Instant::now() + REFRESH_INTERVAL);
    }
}
