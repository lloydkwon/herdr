mod history_read;
mod id;
mod runtime;
mod runtime_registry;
pub mod state;
mod title;

pub(crate) use history_read::{merge_scrolled_up, snapshot_text, ScreenSnapshot, UpwardMerge};
pub use id::TerminalId;
pub use runtime::TerminalRuntime;
pub(crate) use runtime_registry::TerminalRuntimeRegistry;
// 훅 권한은 유닉스 전용 핸드오프 캐리어만 참조하므로 윈도우 빌드에서 unused 가 되지 않게 게이트한다.
#[cfg(unix)]
pub use state::HookAuthority;
pub use state::{
    AgentMetadataReport, EffectivePresentation, EffectiveStateChange, HandoffAgentRestore,
    TerminalState, TerminalStateMutation,
};
pub(crate) use title::stripped_terminal_title;
