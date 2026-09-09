#[cfg(unix)]
use serde::{Deserialize, Serialize};

/// Long-lived pane runtime transferred during server replacement.
///
/// Handoff preserves server-owned session state such as PTYs, processes, agent
/// identity, and durable plugin/session metadata. It intentionally does not
/// preserve transient coordination such as in-flight requests, waits,
/// subscriptions, client sockets, or pane-to-pane messages; clients reconnect
/// and retry those operations after replacement.
#[cfg(unix)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct HandoffRuntimeState {
    pub pane_id: u32,
    pub child_pid: u32,
    pub rows: u16,
    pub cols: u16,
    pub cell_width_px: u32,
    pub cell_height_px: u32,
    #[serde(default)]
    pub keyboard_protocol_flags: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyboard_protocol_ansi: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_state: Option<crate::pane::InputState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_history_ansi: Option<String>,
    /// 핸드오프 시점의 에이전트 상태 라벨(`idle` / `working` / `blocked`). 구버전 manifest 에는 없다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_state: Option<String>,
    /// 핸드오프 시점 에이전트 상태의 전이 시각(unix ms). 구버전 manifest 에는 없다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_state_changed_at_unix_ms: Option<u64>,
    /// 핸드오프 시점에 화면/프로세스로 감지한 에이전트 라벨. 구버전 manifest 에는 없다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detected_agent: Option<String>,
    /// 핸드오프 시점의 훅 권한. 이것이 없으면 가져온 직후 프로세스 재감지가 상태를
    /// `unknown` 으로 되돌려 전이 시각이 리셋된다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_hook_authority: Option<HandoffHookAuthority>,
}

/// `TerminalState::hook_authority` 의 직렬화 형태. `Instant` 는 넘길 수 없으므로
/// 보고 시각은 핸드오프 시점 기준 경과(ms)로 싣는다.
#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HandoffHookAuthority {
    pub source: String,
    pub agent_label: String,
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_kind: Option<crate::agent_resume::AgentSessionRefKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_value: Option<String>,
    #[serde(default)]
    pub reported_age_ms: u64,
}

#[cfg(unix)]
impl HandoffRuntimeState {
    pub fn with_pane_id(mut self, pane_id: crate::layout::PaneId) -> Self {
        self.pane_id = pane_id.raw();
        self
    }

    /// 핸드오프로 넘길 상태 라벨. `Unknown` 은 넘길 정보가 없으므로 None 이다.
    pub fn agent_state_label(state: crate::detect::AgentState) -> Option<&'static str> {
        match state {
            crate::detect::AgentState::Idle => Some("idle"),
            crate::detect::AgentState::Working => Some("working"),
            crate::detect::AgentState::Blocked => Some("blocked"),
            crate::detect::AgentState::Unknown => None,
        }
    }

    /// `agent_state_label` 의 역변환. 모르는 라벨은 None 으로 무시한다.
    pub fn parse_agent_state_label(label: &str) -> Option<crate::detect::AgentState> {
        match label {
            "idle" => Some(crate::detect::AgentState::Idle),
            "working" => Some(crate::detect::AgentState::Working),
            "blocked" => Some(crate::detect::AgentState::Blocked),
            _ => None,
        }
    }

    /// 내보내는 서버가 터미널의 에이전트 상태·전이 시각·감지 에이전트·훅 권한을 싣는다.
    pub fn record_agent_state(
        &mut self,
        terminal: &crate::terminal::TerminalState,
        now: std::time::Instant,
    ) {
        self.agent_state = Self::agent_state_label(terminal.state).map(str::to_owned);
        self.agent_state_changed_at_unix_ms = terminal.last_agent_state_changed_at_unix_ms;
        self.detected_agent = terminal
            .detected_agent
            .map(|agent| crate::detect::agent_label(agent).to_owned());
        self.agent_hook_authority =
            terminal
                .hook_authority
                .as_ref()
                .map(|authority| HandoffHookAuthority {
                    source: authority.source.clone(),
                    agent_label: authority.agent_label.clone(),
                    state: Self::agent_state_label(authority.state)
                        .unwrap_or("unknown")
                        .to_owned(),
                    message: authority.message.clone(),
                    session_kind: authority.session_ref.as_ref().map(|session| session.kind),
                    session_value: authority
                        .session_ref
                        .as_ref()
                        .map(|session| session.value.clone()),
                    reported_age_ms: now
                        .saturating_duration_since(authority.reported_at)
                        .as_millis()
                        .min(u128::from(u64::MAX)) as u64,
                });
    }

    /// 가져오는 서버가 복원할 에이전트 상태. 넘어온 에이전트 정보가 전혀 없으면 None.
    pub fn restored_agent_state(
        &self,
        now: std::time::Instant,
    ) -> Option<crate::terminal::HandoffAgentRestore> {
        let detected_agent = self
            .detected_agent
            .as_deref()
            .and_then(crate::detect::parse_agent_label);
        let hook_authority = self.agent_hook_authority.as_ref().and_then(|hook| {
            let state = Self::parse_agent_state_label(&hook.state)?;
            let session_ref = match (hook.session_kind, hook.session_value.as_deref()) {
                (Some(crate::agent_resume::AgentSessionRefKind::Id), Some(value)) => {
                    crate::agent_resume::AgentSessionRef::id(value)
                }
                (Some(crate::agent_resume::AgentSessionRefKind::Path), Some(value)) => {
                    crate::agent_resume::AgentSessionRef::path(value)
                }
                _ => None,
            };
            Some(crate::terminal::HookAuthority {
                source: hook.source.clone(),
                agent_label: hook.agent_label.clone(),
                state,
                message: hook.message.clone(),
                reported_at: now
                    .checked_sub(std::time::Duration::from_millis(hook.reported_age_ms))
                    .unwrap_or(now),
                session_ref,
            })
        });
        if detected_agent.is_none() && hook_authority.is_none() {
            return None;
        }
        Some(crate::terminal::HandoffAgentRestore {
            detected_agent,
            state: self
                .agent_state
                .as_deref()
                .and_then(Self::parse_agent_state_label)
                .unwrap_or(crate::detect::AgentState::Unknown),
            hook_authority,
            changed_at_unix_ms: self.agent_state_changed_at_unix_ms,
        })
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::detect::{Agent, AgentState};
    use std::time::{Duration, Instant};

    fn runtime_state() -> HandoffRuntimeState {
        HandoffRuntimeState {
            pane_id: 1,
            child_pid: 2,
            rows: 24,
            cols: 80,
            cell_width_px: 8,
            cell_height_px: 16,
            keyboard_protocol_flags: 0,
            keyboard_protocol_ansi: None,
            input_state: None,
            terminal_title: None,
            initial_history_ansi: None,
            agent_state: None,
            agent_state_changed_at_unix_ms: None,
            detected_agent: None,
            agent_hook_authority: None,
        }
    }

    #[test]
    fn agent_state_round_trips_through_manifest_json() {
        let now = Instant::now();
        let mut terminal = crate::terminal::TerminalState::new(
            crate::terminal::TerminalId::alloc(),
            "/tmp".into(),
        );
        terminal.detected_agent = Some(Agent::Claude);
        terminal.state = AgentState::Working;
        terminal.last_agent_state_changed_at_unix_ms = Some(1_234);
        terminal.hook_authority = Some(crate::terminal::HookAuthority {
            source: "herdr:claude".into(),
            agent_label: "claude".into(),
            state: AgentState::Working,
            message: Some("thinking".into()),
            reported_at: now - Duration::from_secs(5),
            session_ref: crate::agent_resume::AgentSessionRef::id(
                "c20cd52f-c488-47fe-85ee-b1ef1c85cca3",
            ),
        });

        let mut state = runtime_state();
        state.record_agent_state(&terminal, now);
        let json = serde_json::to_string(&state).unwrap();
        let decoded: HandoffRuntimeState = serde_json::from_str(&json).unwrap();
        let later = now + Duration::from_secs(1);
        let restore = decoded
            .restored_agent_state(later)
            .expect("carried agent state");

        assert_eq!(restore.detected_agent, Some(Agent::Claude));
        assert_eq!(restore.state, AgentState::Working);
        assert_eq!(restore.changed_at_unix_ms, Some(1_234));
        let hook = restore.hook_authority.expect("hook authority carried");
        assert_eq!(hook.source, "herdr:claude");
        assert_eq!(hook.state, AgentState::Working);
        assert_eq!(hook.message.as_deref(), Some("thinking"));
        assert_eq!(
            hook.session_ref.map(|session| session.value),
            Some("c20cd52f-c488-47fe-85ee-b1ef1c85cca3".into())
        );
        // 보고 시각은 경과(ms)로 넘어가므로 가져온 쪽에서도 5초 전으로 복원된다.
        assert_eq!(
            later.saturating_duration_since(hook.reported_at),
            Duration::from_secs(5)
        );
    }

    #[test]
    fn manifest_without_agent_fields_restores_nothing() {
        let json = serde_json::to_string(&runtime_state()).unwrap();
        let decoded: HandoffRuntimeState = serde_json::from_str(&json).unwrap();
        assert!(decoded.restored_agent_state(Instant::now()).is_none());
    }
}

#[derive(Debug)]
pub(crate) struct ImportedHandoffRuntime {
    #[cfg(unix)]
    pub master_fd: std::os::fd::RawFd,
    #[cfg(unix)]
    pub state: HandoffRuntimeState,
}
