//! 에이전트 상태 경과시간 라벨 포맷.
//!
//! 사이드바 폭이 좁으므로 `12초` / `3분` / `1시간 20분` 처럼 짧게 만든다.

const MS_PER_SECOND: u64 = 1_000;
const MS_PER_MINUTE: u64 = 60 * MS_PER_SECOND;
const MS_PER_HOUR: u64 = 60 * MS_PER_MINUTE;

/// 경과 밀리초를 짧은 한국어 라벨로 바꾼다.
///
/// 1분 미만은 초, 1시간 미만은 분, 그 이상은 `N시간` 뒤에 0이 아닌 분만 덧붙인다.
pub(crate) fn format_elapsed_label(elapsed_ms: u64) -> String {
    if elapsed_ms < MS_PER_MINUTE {
        return format!("{}초", elapsed_ms / MS_PER_SECOND);
    }
    if elapsed_ms < MS_PER_HOUR {
        return format!("{}분", elapsed_ms / MS_PER_MINUTE);
    }
    let hours = elapsed_ms / MS_PER_HOUR;
    let minutes = (elapsed_ms % MS_PER_HOUR) / MS_PER_MINUTE;
    if minutes == 0 {
        format!("{hours}시간")
    } else {
        format!("{hours}시간 {minutes}분")
    }
}

/// `changed_at_unix_ms` 이후 라벨 문자열이 다음으로 바뀌는 unix ms 시각.
///
/// 1분 미만이면 다음 초 경계, 그 이상이면 다음 분 경계다. 렌더 갱신 타이머가 이 값으로
/// 정확히 필요한 순간에만 다시 그린다.
pub(crate) fn next_elapsed_label_change_unix_ms(changed_at_unix_ms: u64, now_unix_ms: u64) -> u64 {
    let elapsed = crate::clock::elapsed_ms(changed_at_unix_ms, now_unix_ms);
    let unit = if elapsed < MS_PER_MINUTE {
        MS_PER_SECOND
    } else {
        MS_PER_MINUTE
    };
    changed_at_unix_ms.saturating_add((elapsed / unit + 1).saturating_mul(unit))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_seconds_minutes_and_hours() {
        assert_eq!(format_elapsed_label(0), "0초");
        assert_eq!(format_elapsed_label(12_500), "12초");
        assert_eq!(format_elapsed_label(59_999), "59초");
        assert_eq!(format_elapsed_label(60_000), "1분");
        assert_eq!(format_elapsed_label(180_000), "3분");
        assert_eq!(format_elapsed_label(3_600_000), "1시간");
        assert_eq!(format_elapsed_label(4_800_000), "1시간 20분");
    }

    #[test]
    fn next_change_is_next_second_under_a_minute_and_next_minute_after() {
        assert_eq!(next_elapsed_label_change_unix_ms(1_000, 1_000), 2_000);
        assert_eq!(next_elapsed_label_change_unix_ms(1_000, 13_400), 14_000);
        assert_eq!(next_elapsed_label_change_unix_ms(1_000, 61_000), 121_000);
        assert_eq!(next_elapsed_label_change_unix_ms(1_000, 181_500), 241_000);
    }

    #[test]
    fn next_change_after_clock_went_backwards_is_one_second_after_the_change() {
        // 미래 시각(시계 역행)이면 경과 0 으로 보고 전이 시각 + 1초를 돌려준다.
        assert_eq!(next_elapsed_label_change_unix_ms(5_000, 2_000), 6_000);
    }
}
