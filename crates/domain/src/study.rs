//! Study time: how long each document was read, per day, saved in
//! `%APPDATA%\leme\study.txt`.
//!
//! Time is counted from activity, not with a running timer: each key,
//! scroll, or click adds the time since the previous one, unless that gap
//! was long enough that the reader was likely away. Nothing runs while the
//! app sits idle.
//!
//! Format, one entry per line: `YYYY-MM-DD\t<seconds>\t<path>`, with local
//! dates. Unknown lines are ignored; entries older than a year are dropped.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::annotations::date;
use crate::config::{data_dir, write_atomic};

/// A pause longer than this means the reader stepped away; it isn't counted.
pub const IDLE_SECONDS: u64 = 5 * 60;
/// Days of history kept.
const KEEP_DAYS: i64 = 366;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct StudyLog {
    /// (local day number, document) -> seconds.
    entries: BTreeMap<(i64, PathBuf), u64>,
}

impl StudyLog {
    pub fn parse(text: &str, today: i64) -> Self {
        let mut log = Self::default();
        for line in text.lines() {
            let mut parts = line.splitn(3, '\t');
            let (Some(day), Some(secs), Some(path)) = (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let (Some(day), Ok(secs)) = (parse_date(day), secs.trim().parse::<u64>()) else {
                continue;
            };
            if today - day < KEEP_DAYS && !path.is_empty() {
                log.add(day, Path::new(path), secs);
            }
        }
        log
    }

    pub fn serialize(&self) -> String {
        self.entries
            .iter()
            .map(|((day, path), secs)| format!("{}\t{secs}\t{}\n", date_of(*day), path.display()))
            .collect()
    }

    pub fn add(&mut self, day: i64, path: &Path, seconds: u64) {
        *self.entries.entry((day, path.to_owned())).or_default() += seconds;
    }

    /// Seconds on days `from..=to`, for every document.
    pub fn total(&self, from: i64, to: i64) -> u64 {
        self.entries
            .range((from, PathBuf::new())..)
            .take_while(|((day, _), _)| *day <= to)
            .map(|(_, secs)| secs)
            .sum()
    }

    /// Seconds spent on one document, ever.
    pub fn for_path(&self, path: &Path) -> u64 {
        self.entries
            .iter()
            .filter(|((_, p), _)| p == path)
            .map(|(_, secs)| secs)
            .sum()
    }

    /// Moves study seconds recorded under `from` to `to` (for renamed/moved PDFs).
    pub fn migrate_path(&mut self, from: &Path, to: &Path) -> bool {
        if from == to {
            return false;
        }
        let keys: Vec<(i64, PathBuf)> = self
            .entries
            .keys()
            .filter(|(_, p)| p == from)
            .cloned()
            .collect();
        if keys.is_empty() {
            return false;
        }
        for key in keys {
            if let Some(secs) = self.entries.remove(&key) {
                self.add(key.0, to, secs);
            }
        }
        true
    }

    pub fn load() -> Self {
        file_path()
            .and_then(|path| fs::read_to_string(path).ok())
            .map(|text| Self::parse(&text, today()))
            .unwrap_or_default()
    }

    pub fn save(&self) -> io::Result<()> {
        let path = file_path().ok_or_else(|| io::Error::other("APPDATA is not set"))?;
        write_atomic(&path, &self.serialize())
    }
}

/// Turns activity into study seconds.
#[derive(Debug, Default)]
pub struct Clock {
    last: Option<u64>,
    pub paused: bool,
}

impl Clock {
    /// Seconds to count for activity at `now` (Unix seconds).
    pub fn tick(&mut self, now: u64) -> u64 {
        let previous = self.last.replace(now);
        if self.paused {
            return 0;
        }
        match previous {
            Some(last) if now >= last && now - last <= IDLE_SECONDS => now - last,
            _ => 0,
        }
    }

    /// Forgets the last activity, so a gap that follows (a tab switch or
    /// resuming) isn't counted.
    pub fn reset(&mut self) {
        self.last = None;
    }
}

/// "1h 05m", "12m", or "0m".
pub fn format_duration(seconds: u64) -> String {
    let minutes = seconds / 60;
    match minutes / 60 {
        0 => format!("{minutes}m"),
        hours => format!("{hours}h {:02}m", minutes % 60),
    }
}

fn date_of(day: i64) -> String {
    date(u64::try_from(day).unwrap_or(0) * 86_400)
}

/// Days since 1970-01-01 for `YYYY-MM-DD`.
fn parse_date(text: &str) -> Option<i64> {
    let mut parts = text.trim().splitn(3, '-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(days_from_civil(year, month, day))
}

/// Howard Hinnant's days-from-civil, the inverse of `annotations::date`.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Today's local day number.
pub fn today() -> i64 {
    let now = i64::try_from(crate::annotations::now()).unwrap_or(0);
    (now + local_offset()).div_euclid(86_400)
}

/// The hour of the day on the local clock, 0–23.
pub fn local_hour() -> u32 {
    let now = i64::try_from(crate::annotations::now()).unwrap_or(0);
    hour_of(now + local_offset())
}

fn hour_of(local_seconds: i64) -> u32 {
    (local_seconds.rem_euclid(86_400) / 3600) as u32
}

/// Seconds the local clock is ahead of UTC.
#[cfg(unix)]
fn local_offset() -> i64 {
    // `struct tm` as glibc and musl define it.
    #[repr(C)]
    struct Tm {
        fields: [i32; 9],
        gmtoff: core::ffi::c_long,
        zone: *const core::ffi::c_char,
    }

    extern "C" {
        fn time(t: *mut i64) -> i64;
        fn localtime_r(t: *const i64, out: *mut Tm) -> *mut Tm;
    }

    let mut tm = Tm {
        fields: [0; 9],
        gmtoff: 0,
        zone: core::ptr::null(),
    };
    // SAFETY: `time` accepts null; `localtime_r` only writes a `struct tm`,
    // which `Tm` matches, into memory we own, and returns null on failure.
    unsafe {
        let now = time(core::ptr::null_mut());
        if localtime_r(&now, &mut tm).is_null() {
            return 0;
        }
    }
    // c_long is i64 on 64-bit Linux but not on every unix.
    #[allow(clippy::useless_conversion)]
    i64::from(tm.gmtoff)
}

/// Seconds the local clock is ahead of UTC.
#[cfg(windows)]
fn local_offset() -> i64 {
    #[repr(C)]
    #[derive(Default)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        milliseconds: u16,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetLocalTime(time: *mut SystemTime);
        fn GetSystemTime(time: *mut SystemTime);
    }

    let mut local = SystemTime::default();
    let mut utc = SystemTime::default();
    // SAFETY: both functions only write a SYSTEMTIME, which `SystemTime`
    // matches field for field, into memory we own.
    unsafe {
        GetLocalTime(&mut local);
        GetSystemTime(&mut utc);
    }
    let seconds = |t: &SystemTime| {
        days_from_civil(t.year.into(), t.month.into(), t.day.into()) * 86_400
            + i64::from(t.hour) * 3600
            + i64::from(t.minute) * 60
            + i64::from(t.second)
    };
    // Whole minutes: the two calls may straddle a second or minute boundary.
    ((seconds(&local) - seconds(&utc) + 30).div_euclid(60)) * 60
}

fn file_path() -> Option<PathBuf> {
    Some(data_dir()?.join("study.txt"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_totals_and_round_trip() {
        let mut log = StudyLog::default();
        let a = Path::new("C:\\a.pdf");
        let b = Path::new("C:\\b.pdf");
        log.add(100, a, 60);
        log.add(100, a, 30);
        log.add(101, b, 600);
        log.add(95, b, 5);
        assert_eq!(log.total(100, 100), 90);
        assert_eq!(log.total(95, 101), 695);
        assert_eq!(log.total(102, 110), 0);
        assert_eq!(log.for_path(b), 605);
        assert_eq!(StudyLog::parse(&log.serialize(), 101), log);
    }

    #[test]
    fn old_and_broken_lines_are_dropped() {
        let text = "1970-01-02\t10\tC:\\old.pdf\ngarbage\n2026-11-10\tx\tC:\\a.pdf\n2026-11-10\t7\tC:\\a.pdf\n";
        let log = StudyLog::parse(text, 20_767);
        assert_eq!(log.total(0, 30_000), 7);
    }

    #[test]
    fn clock_skips_long_gaps_and_pauses() {
        let mut clock = Clock::default();
        assert_eq!(clock.tick(1000), 0);
        assert_eq!(clock.tick(1010), 10);
        assert_eq!(clock.tick(1010 + IDLE_SECONDS + 1), 0);
        clock.paused = true;
        assert_eq!(clock.tick(2000), 0);
        clock.paused = false;
        clock.reset();
        assert_eq!(clock.tick(2005), 0);
        assert_eq!(clock.tick(2009), 4);
        // A clock that went backwards counts nothing.
        assert_eq!(clock.tick(1500), 0);
    }
}
