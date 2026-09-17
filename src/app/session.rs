use std::time::{Duration, Instant};

use super::{App, SESSION_SAVE_DEBOUNCE, SESSION_SAVE_MAX_DELAY};

enum SessionSaveJob {
    Clear,
    Save {
        snapshot: crate::persist::SessionSnapshot,
        history: Option<crate::persist::SessionHistorySnapshot>,
    },
}

impl App {
    pub(super) fn schedule_session_save(&mut self) {
        self.schedule_session_save_at(Instant::now());
    }

    /// 디바운스(마지막 요청 + 5초)로 저장을 미루되, 첫 요청 + 상한을 넘기지는 않는다.
    pub(crate) fn schedule_session_save_at(&mut self, now: Instant) {
        if self.policy.persist_session {
            self.pane_exit_checkpoint_pending = false;
            let first_requested = *self.session_save_requested_at.get_or_insert(now);
            self.session_save_deadline =
                Some((now + SESSION_SAVE_DEBOUNCE).min(first_requested + SESSION_SAVE_MAX_DELAY));
        }
    }

    pub(crate) fn sync_session_save_schedule(&mut self) {
        if self.state.session_dirty {
            self.state.session_dirty = false;
            self.schedule_session_save();
        }
    }

    fn reap_finished_session_save(&mut self) {
        if self
            .session_save_thread
            .as_ref()
            .is_some_and(std::thread::JoinHandle::is_finished)
        {
            if let Some(thread) = self.session_save_thread.take() {
                let _ = thread.join();
            }
        }
    }

    fn capture_session_save_job(&self) -> SessionSaveJob {
        if self.state.workspaces.is_empty() {
            SessionSaveJob::Clear
        } else {
            let snapshot = crate::persist::capture(
                &self.state.workspaces,
                &self.state.terminals,
                &self.terminal_runtimes,
                self.state.active,
                self.state.selected,
            );
            let history = self.persist_pane_history.then(|| {
                crate::persist::capture_history(&self.state.workspaces, &self.terminal_runtimes)
            });
            SessionSaveJob::Save { snapshot, history }
        }
    }

    pub(crate) fn start_background_session_save(&mut self) {
        if !self.policy.persist_session {
            self.session_save_deadline = None;
            return;
        }

        self.reap_finished_session_save();
        if self.session_save_thread.is_some() {
            self.session_save_deadline = Some(Instant::now() + Duration::from_millis(250));
            return;
        }

        let job = self.capture_session_save_job();
        self.pane_exit_checkpoint_pending = false;
        self.session_save_deadline = None;
        self.session_save_requested_at = None;
        match std::thread::Builder::new()
            .name("herdr-session-save".into())
            .spawn(move || run_session_save_job(job))
        {
            Ok(thread) => self.session_save_thread = Some(thread),
            Err(err) => {
                tracing::warn!(err = %err, "failed to spawn session save thread; saving inline");
                run_session_save_job(self.capture_session_save_job());
            }
        }
    }

    pub(crate) fn save_session_now(&mut self) {
        if let Some(thread) = self.session_save_thread.take() {
            let _ = thread.join();
        }

        if !self.policy.persist_session {
            self.session_save_deadline = None;
            return;
        }

        run_session_save_job(self.capture_session_save_job());
        self.pane_exit_checkpoint_pending = false;
        self.session_save_deadline = None;
        self.session_save_requested_at = None;
    }

    pub(crate) fn checkpoint_session_before_pane_exit(&mut self) {
        if !self.policy.persist_session
            || (self.pane_exit_checkpoint_pending && !self.state.session_dirty)
        {
            return;
        }
        self.save_session_now();
        self.pane_exit_checkpoint_pending = true;
        self.state.session_dirty = false;
    }

    pub(crate) fn finish_checkpointed_pane_exit(&mut self) {
        if self.pane_exit_checkpoint_pending {
            self.state.session_dirty = false;
            let now = Instant::now();
            self.session_save_requested_at.get_or_insert(now);
            self.session_save_deadline = Some(now + SESSION_SAVE_DEBOUNCE);
        }
    }

    pub(crate) fn save_session_on_shutdown(&mut self) {
        if self.pane_exit_checkpoint_pending && !self.state.session_dirty {
            self.session_save_deadline = None;
            return;
        }
        self.save_session_now();
    }
}

fn run_session_save_job(job: SessionSaveJob) {
    match job {
        SessionSaveJob::Clear => crate::persist::clear(),
        SessionSaveJob::Save { snapshot, history } => {
            crate::persist::save(&snapshot, history.as_ref());
        }
    }
}
