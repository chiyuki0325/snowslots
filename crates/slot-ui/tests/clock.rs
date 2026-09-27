use slot_ui::{clock_label, Polaroids};

#[test]
fn recent_states_read_relative_and_old_ones_read_absolute() {
    let now = "2026-08-09_14-36-05";
    assert_eq!(
        Polaroids::relative_time("2026-08-09_14-32-05", now),
        "4 分钟前"
    );
    assert_eq!(
        Polaroids::relative_time("2026-08-09_03-00-00", now),
        "11 小时前"
    );
    // Past twelve hours the relative form stops being useful and starts being vague.
    assert_eq!(
        Polaroids::relative_time("2026-08-08_20-00-00", now),
        "2026年08月08日 20:00"
    );
    assert_eq!(
        Polaroids::relative_time("2025-01-02_09-05-00", now),
        "2025年01月02日 09:05"
    );
}

#[test]
fn the_clock_is_24_hour_and_shows_no_seconds() {
    assert_eq!(
        clock_label("2026-08-09_21-07-00"),
        "21:07",
        "not 24 hour, or showing seconds"
    );
}
