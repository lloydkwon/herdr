//! live handoff 로 끊긴 로컬 연결을 종료 대신 재접속으로 복구하는 순수 정책.
//!
//! 서버는 핸드오프를 시작하며 붙어 있는 클라이언트에 `ServerShutdown` 을 보낸다. 같은 소켓이
//! 교체 서버 아래에서 곧 다시 열리므로, 단일 머신 셸 클라이언트는 그 사유에 한해 종료하지 않고
//! 연합 모드가 쓰는 Local supervisor 경로로 재접속한다. 그 외 사유(detach, `server.stop`, 크래시)는
//! 기존처럼 종료한다.

use std::time::{Duration, Instant};

use super::endpoint::ClientEndpointId;
use crate::protocol::{server_shutdown_reason_kind, ServerShutdownReasonKind};

/// 교체 서버가 이 시간 안에 소켓을 열지 않으면 포기하고 종료한다.
/// `update::SERVER_HANDOFF_CONFIRM_TIMEOUT` 과 같은 값이다.
pub(super) const LOCAL_HANDOFF_RECONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// 핸드오프 재접속을 기다리는 동안의 상태.
pub(super) struct LocalHandoffRecovery {
    deadline: Instant,
    pub(super) reason: Option<String>,
}

impl LocalHandoffRecovery {
    pub(super) fn new(now: Instant, reason: Option<String>) -> Self {
        Self {
            deadline: now + LOCAL_HANDOFF_RECONNECT_TIMEOUT,
            reason,
        }
    }

    pub(super) fn expired(&self, now: Instant) -> bool {
        now >= self.deadline
    }
}

/// 단일 머신 셸 클라이언트가 이 `ServerShutdown` 을 재접속으로 처리해야 하는가.
///
/// 연합 모드는 이미 자체 재접속 경로가 있고, 직접 attach(비셸) 클라이언트는 supervisor 를
/// 돌리지 않으므로 대상이 아니다.
pub(super) fn local_shutdown_should_reconnect(
    federated: bool,
    shell_mode: bool,
    endpoint_id: &ClientEndpointId,
    reason: Option<&str>,
) -> bool {
    !federated
        && shell_mode
        && endpoint_id.is_local()
        && server_shutdown_reason_kind(reason) == ServerShutdownReasonKind::LiveHandoff
}

/// 연결이 끊겼을 때 사용자에게 보일 안내 문구. 핸드오프면 «업데이트 중»으로 표현한다.
pub(super) fn shutdown_disconnect_notice(reason: Option<&str>) -> String {
    match server_shutdown_reason_kind(reason) {
        ServerShutdownReasonKind::LiveHandoff => "server is updating".to_owned(),
        _ => reason
            .map(str::to_owned)
            .unwrap_or_else(|| "server stopped".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{SERVER_SHUTDOWN_REASON_DETACHED, SERVER_SHUTDOWN_REASON_LIVE_HANDOFF};

    #[test]
    fn only_local_shell_clients_reconnect_on_handoff_reason() {
        let local = ClientEndpointId::Local;
        let handoff = Some(SERVER_SHUTDOWN_REASON_LIVE_HANDOFF);
        assert!(local_shutdown_should_reconnect(
            false, true, &local, handoff
        ));
        // 연합 모드·비셸·다른 사유는 대상이 아니다.
        assert!(!local_shutdown_should_reconnect(
            true, true, &local, handoff
        ));
        assert!(!local_shutdown_should_reconnect(
            false, false, &local, handoff
        ));
        assert!(!local_shutdown_should_reconnect(
            false,
            true,
            &local,
            Some(SERVER_SHUTDOWN_REASON_DETACHED)
        ));
        assert!(!local_shutdown_should_reconnect(
            false,
            true,
            &local,
            Some("server is shutting down")
        ));
        assert!(!local_shutdown_should_reconnect(false, true, &local, None));
        let ssh = ClientEndpointId::Ssh(crate::client::endpoint::ProfileId::generate());
        assert!(!local_shutdown_should_reconnect(false, true, &ssh, handoff));
    }

    #[test]
    fn recovery_expires_after_the_timeout() {
        let now = Instant::now();
        let recovery =
            LocalHandoffRecovery::new(now, Some(SERVER_SHUTDOWN_REASON_LIVE_HANDOFF.into()));
        assert!(!recovery.expired(now + LOCAL_HANDOFF_RECONNECT_TIMEOUT - Duration::from_millis(1)));
        assert!(recovery.expired(now + LOCAL_HANDOFF_RECONNECT_TIMEOUT));
    }

    #[test]
    fn disconnect_notice_names_the_update_and_keeps_other_reasons() {
        assert_eq!(
            shutdown_disconnect_notice(Some(SERVER_SHUTDOWN_REASON_LIVE_HANDOFF)),
            "server is updating"
        );
        assert_eq!(
            shutdown_disconnect_notice(Some("server is shutting down")),
            "server is shutting down"
        );
        assert_eq!(shutdown_disconnect_notice(None), "server stopped");
    }
}
