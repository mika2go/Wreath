use std::time::{SystemTime, UNIX_EPOCH};

const MONTHS: [&str; 12] = [
    "Januar",
    "Februar",
    "März",
    "April",
    "Mai",
    "Juni",
    "Juli",
    "August",
    "September",
    "Oktober",
    "November",
    "Dezember",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Civil {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
}

impl Civil {
    pub fn day_index(self) -> i64 {
        days_from_civil(self.year, self.month, self.day)
    }
}

pub fn now() -> Civil {
    local(SystemTime::now())
}

pub fn local(time: SystemTime) -> Civil {
    #[cfg(target_os = "windows")]
    if let Some(civil) = windows_local(time) {
        return civil;
    }
    utc(time)
}

pub fn day_label(clip: Civil, today: Civil) -> String {
    match today.day_index() - clip.day_index() {
        0 => "Heute".to_owned(),
        1 => "Gestern".to_owned(),
        _ => {
            let month = MONTHS[(clip.month.clamp(1, 12) - 1) as usize];
            if clip.year == today.year {
                format!("{}. {month}", clip.day)
            } else {
                format!("{}. {month} {}", clip.day, clip.year)
            }
        }
    }
}

pub fn time_label(civil: Civil) -> String {
    format!("{:02}:{:02}", civil.hour, civil.minute)
}

pub fn stamp_label(clip: Civil, today: Civil) -> String {
    format!("{}, {}", day_label(clip, today), time_label(clip))
}

pub fn within_days(clip: Civil, today: Civil, days: i64) -> bool {
    let elapsed = today.day_index() - clip.day_index();
    (0..days).contains(&elapsed) || elapsed < 0
}

pub fn same_month(clip: Civil, today: Civil) -> bool {
    clip.year == today.year && clip.month == today.month
}

fn utc(time: SystemTime) -> Civil {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64);
    civil_from_unix(seconds)
}

fn civil_from_unix(seconds: i64) -> Civil {
    let days = seconds.div_euclid(86_400);
    let remainder = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    Civil {
        year,
        month,
        day,
        hour: (remainder / 3_600) as u8,
        minute: (remainder % 3_600 / 60) as u8,
    }
}

fn days_from_civil(year: i32, month: u8, day: u8) -> i64 {
    let month = i64::from(month.clamp(1, 12));
    let year = i64::from(year) - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> (i32, u8, u8) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = shifted_month + if shifted_month < 10 { 3 } else { -9 };
    (
        (year + i64::from(month <= 2)) as i32,
        month as u8,
        day as u8,
    )
}

#[cfg(target_os = "windows")]
fn windows_local(time: SystemTime) -> Option<Civil> {
    use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};

    const UNIX_EPOCH_IN_FILETIME: u64 = 116_444_736_000_000_000;

    let elapsed = time.duration_since(UNIX_EPOCH).ok()?;
    let ticks = UNIX_EPOCH_IN_FILETIME.checked_add(elapsed.as_nanos() as u64 / 100)?;
    let file_time = FILETIME {
        dwLowDateTime: (ticks & 0xffff_ffff) as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut universal = SYSTEMTIME::default();
    unsafe { FileTimeToSystemTime(&file_time, &mut universal) }.ok()?;
    let mut civil = SYSTEMTIME::default();
    unsafe { SystemTimeToTzSpecificLocalTime(None, &universal, &mut civil) }.ok()?;
    Some(Civil {
        year: i32::from(civil.wYear),
        month: civil.wMonth as u8,
        day: civil.wDay as u8,
        hour: civil.wHour as u8,
        minute: civil.wMinute as u8,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn civil(year: i32, month: u8, day: u8, hour: u8, minute: u8) -> Civil {
        Civil {
            year,
            month,
            day,
            hour,
            minute,
        }
    }

    #[test]
    fn unix_seconds_become_a_calendar_date() {
        assert_eq!(civil_from_unix(0), civil(1970, 1, 1, 0, 0));
        assert_eq!(civil_from_unix(1_755_527_700), civil(2025, 8, 18, 14, 35));
        assert_eq!(civil_from_unix(1_582_934_400), civil(2020, 2, 29, 0, 0));
    }

    #[test]
    fn a_date_round_trips_through_its_day_index() {
        for seconds in [0_i64, 1_000_000_000, 1_755_527_700, 4_102_444_800] {
            let value = civil_from_unix(seconds);
            let (year, month, day) = civil_from_days(value.day_index());
            assert_eq!((year, month, day), (value.year, value.month, value.day));
        }
    }

    #[test]
    fn consecutive_days_are_labelled_relatively() {
        let today = civil(2026, 8, 18, 10, 7);

        assert_eq!(day_label(today, today), "Heute");
        assert_eq!(day_label(civil(2026, 8, 17, 23, 59), today), "Gestern");
        assert_eq!(day_label(civil(2026, 8, 16, 9, 0), today), "16. August");
        assert_eq!(
            day_label(civil(2025, 12, 24, 9, 0), today),
            "24. Dezember 2025"
        );
    }

    #[test]
    fn a_clip_stamp_pairs_the_day_with_the_local_time() {
        let today = civil(2026, 8, 18, 10, 7);

        assert_eq!(
            stamp_label(civil(2026, 8, 18, 14, 35), today),
            "Heute, 14:35"
        );
        assert_eq!(
            stamp_label(civil(2026, 8, 17, 9, 8), today),
            "Gestern, 09:08"
        );
    }

    #[test]
    fn time_ranges_cover_the_current_day_week_and_month() {
        let today = civil(2026, 8, 18, 10, 7);

        assert!(within_days(civil(2026, 8, 18, 0, 1), today, 1));
        assert!(!within_days(civil(2026, 8, 17, 23, 59), today, 1));
        assert!(within_days(civil(2026, 8, 12, 8, 0), today, 7));
        assert!(!within_days(civil(2026, 8, 11, 8, 0), today, 7));
        assert!(same_month(civil(2026, 8, 1, 0, 0), today));
        assert!(!same_month(civil(2026, 7, 31, 23, 0), today));
    }

    #[test]
    fn clips_dated_in_the_future_stay_visible() {
        let today = civil(2026, 8, 18, 10, 7);

        assert!(within_days(civil(2026, 8, 19, 10, 0), today, 1));
    }
}
