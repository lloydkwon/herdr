//! 벽시계(unix ms) 헬퍼.
//!
//! 프로세스 로컬 `Instant` 는 직렬화가 안 되므로, 소켓 API·핸드오프를 넘어 살아야 하는
//! 시각은 unix milliseconds(u64) 로 기록한다.

use std::time::{SystemTime, UNIX_EPOCH};

/// 현재 시각을 unix milliseconds 로 반환한다. 시계를 읽을 수 없으면 0.
pub(crate) fn unix_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

/// `since_unix_ms` 이후 경과한 밀리초. 시계가 뒤로 가면(미래 시각) 0 으로 클램프한다.
pub(crate) fn elapsed_ms(since_unix_ms: u64, now_unix_ms: u64) -> u64 {
    now_unix_ms.saturating_sub(since_unix_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_clamps_clock_going_backwards_to_zero() {
        assert_eq!(elapsed_ms(2_000, 1_000), 0);
    }

    #[test]
    fn elapsed_is_difference_when_clock_moves_forward() {
        assert_eq!(elapsed_ms(1_000, 4_000), 3_000);
    }

    #[test]
    fn unix_ms_now_is_after_2024() {
        assert!(unix_ms_now() > 1_704_067_200_000);
    }
}
