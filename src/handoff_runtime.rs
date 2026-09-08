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
    /// 핸드오프 시점의 에이전트 상태 라벨(`idle` / `working`). 구버전 manifest 에는 없다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_state: Option<String>,
    /// 핸드오프 시점 에이전트 상태의 전이 시각(unix ms). 구버전 manifest 에는 없다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_state_changed_at_unix_ms: Option<u64>,
}

#[cfg(unix)]
impl HandoffRuntimeState {
    pub fn with_pane_id(mut self, pane_id: crate::layout::PaneId) -> Self {
        self.pane_id = pane_id.raw();
        self
    }

    /// 핸드오프로 넘길 상태 라벨. `Blocked` 는 화면 신호(visible_blocker) 없이 복원할 수 없고
    /// `Unknown` 은 넘길 정보가 없으므로 둘 다 None 이다.
    pub fn agent_state_label(state: crate::detect::AgentState) -> Option<&'static str> {
        match state {
            crate::detect::AgentState::Idle => Some("idle"),
            crate::detect::AgentState::Working => Some("working"),
            crate::detect::AgentState::Blocked | crate::detect::AgentState::Unknown => None,
        }
    }

    /// `agent_state_label` 의 역변환. 모르는 라벨은 None 으로 무시한다.
    pub fn parse_agent_state_label(label: &str) -> Option<crate::detect::AgentState> {
        match label {
            "idle" => Some(crate::detect::AgentState::Idle),
            "working" => Some(crate::detect::AgentState::Working),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub(crate) struct ImportedHandoffRuntime {
    #[cfg(unix)]
    pub master_fd: std::os::fd::RawFd,
    #[cfg(unix)]
    pub state: HandoffRuntimeState,
}
