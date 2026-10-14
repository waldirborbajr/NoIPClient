use std::{
    fs,
    process,
    time::{SystemTime, UNIX_EPOCH},
};

/// Fisher–Yates using time as a cheap source of randomness (no `rand` dep).
pub(crate) fn shuffle<T>(v: &mut [T]) {
    let mut seed = now_secs() ^ (process::id() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    for i in (1..v.len()).rev() {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let j = (seed as usize) % (i + 1);
        v.swap(i, j);
    }
}
pub(crate) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(crate) fn human_duration(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3_599 => format!("{}m {}s", secs / 60, secs % 60),
        3_600..=86_399 => format!("{}h {}m", secs / 3_600, (secs % 3_600) / 60),
        _ => format!("{}d {}h", secs / 86_400, (secs % 86_400) / 3_600),
    }
}

pub(crate) fn machine_name() -> String {
    fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".into())
}

pub(crate) fn utc_now() -> String {
    let secs = now_secs() as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_duration_uses_the_largest_sensible_units() {
        assert_eq!(human_duration(0), "0s");
        assert_eq!(human_duration(59), "59s");
        assert_eq!(human_duration(60), "1m 0s");
        assert_eq!(human_duration(3_599), "59m 59s");
        assert_eq!(human_duration(3_600), "1h 0m");
        assert_eq!(human_duration(86_399), "23h 59m");
        assert_eq!(human_duration(86_400), "1d 0h");
        assert_eq!(human_duration(90_061), "1d 1h");
    }

    #[test]
    fn utc_now_has_the_expected_shape() {
        let s = utc_now();
        // "YYYY-MM-DD HH:MM:SS UTC"
        assert_eq!(s.len(), 23, "unexpected format: {s}");
        assert!(s.ends_with(" UTC"));
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[7..8], "-");
        assert_eq!(&s[13..14], ":");
        assert_eq!(&s[16..17], ":");
    }
}
