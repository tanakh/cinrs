//! The time of translation, which `__DATE__`, `__TIME__` and `__TIMESTAMP__`
//! say (C17 6.10.8.1, and GCC's `__TIMESTAMP__`).
//!
//! As in GCC:
//!
//! * `SOURCE_DATE_EPOCH`, when it is set, is the moment of translation, in
//!   UTC — what a reproducible build sets. It has to be a decimal number of
//!   seconds from 0 to 253402300799 (the last second of 9999); anything else
//!   is GCC's error, reported where `__DATE__` or `__TIME__` is used.
//! * Otherwise it is now, in local time.
//! * `__TIMESTAMP__` is when the file being read was last modified, in local
//!   time, whatever `SOURCE_DATE_EPOCH` says. Text that is no file of its own
//!   — a `c99!` block's, standard input — takes the moment of translation.
//!
//! Local time is the zone `TZ` names, or `/etc/localtime` — a TZif file
//! (RFC 8536) and the POSIX rule at its end, read here rather than through
//! the C library — and UTC where neither says anything, which is every
//! platform without `/etc/localtime`: Windows.
//!
//! The calendar arithmetic is Howard Hinnant's `days_from_civil` and
//! `civil_from_days`, proleptic Gregorian and exact for every year this needs.

use std::path::Path;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// A moment, and the offset from UTC of the time zone it is told in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalTime {
    /// Seconds since 1970-01-01T00:00:00Z.
    pub seconds: i64,
    /// Seconds east of UTC.
    pub utc_offset: i64,
}

/// The largest `SOURCE_DATE_EPOCH` GCC takes: 9999-12-31T23:59:59Z.
pub const MAX_SOURCE_DATE_EPOCH: i64 = 253_402_300_799;

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

impl LocalTime {
    /// `seconds`, told in UTC.
    pub fn utc(seconds: i64) -> Self {
        Self {
            seconds,
            utc_offset: 0,
        }
    }

    /// `seconds`, told in the local time zone.
    pub fn local(seconds: i64) -> Self {
        Self {
            seconds,
            utc_offset: local_offset(seconds),
        }
    }

    /// The broken-down local time: year, month (1–12), day, weekday
    /// (0 = Sunday), and the second of the day.
    fn civil(self) -> (i64, u32, u32, usize, i64) {
        let local = self.seconds + self.utc_offset;
        let days = local.div_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        let weekday = (days + 4).rem_euclid(7) as usize;
        (year, month, day, weekday, local.rem_euclid(86_400))
    }

    /// `__DATE__`: `"Oct  8 2026"`, the day padded with a space.
    pub fn date(self) -> String {
        let (year, month, day, _, _) = self.civil();
        format!("{} {day:2} {year:4}", MONTHS[month as usize - 1])
    }

    /// `__TIME__`: `"17:45:37"`.
    pub fn time(self) -> String {
        let (_, _, _, _, second) = self.civil();
        clock_string(second)
    }

    /// `__TIMESTAMP__`: `"Thu Oct  8 17:45:35 2026"`, which is `asctime`'s.
    pub fn timestamp(self) -> String {
        let (year, month, day, weekday, second) = self.civil();
        format!(
            "{} {} {day:2} {} {year:4}",
            WEEKDAYS[weekday],
            MONTHS[month as usize - 1],
            clock_string(second)
        )
    }
}

fn clock_string(second: i64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        second / 3600,
        second / 60 % 60,
        second % 60
    )
}

/// The moment of translation: `SOURCE_DATE_EPOCH` in UTC when it is set, or
/// now in local time; GCC's error when the variable says nothing it takes.
pub fn translation_time() -> Result<LocalTime, String> {
    match std::env::var_os("SOURCE_DATE_EPOCH") {
        Some(value) => source_date_epoch(&value.to_string_lossy()).map(LocalTime::utc),
        None => Ok(LocalTime::local(now())),
    }
}

/// Seconds since the epoch, now.
pub fn now() -> i64 {
    seconds_of(SystemTime::now())
}

/// Seconds since the epoch at `time`.
pub fn seconds_of(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(after) => after.as_secs() as i64,
        Err(before) => -(before.duration().as_secs() as i64),
    }
}

/// When the file at `path` was last modified, if it is a file on disk.
pub fn modified(path: &Path) -> Option<LocalTime> {
    let time = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(LocalTime::local(seconds_of(time)))
}

/// Reads `SOURCE_DATE_EPOCH` as GCC does — `strtoll`, so leading white space
/// and a sign are taken, and nothing may follow the digits — and checks it is
/// a moment GCC can print.
pub fn source_date_epoch(text: &str) -> Result<i64, String> {
    let error = || {
        format!(
            "environment variable 'SOURCE_DATE_EPOCH' must expand to a non-negative integer \
             less than or equal to {MAX_SOURCE_DATE_EPOCH}"
        )
    };
    let trimmed = text.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']);
    let (negative, digits) = match trimmed.as_bytes().first() {
        Some(b'-') => (true, &trimmed[1..]),
        Some(b'+') => (false, &trimmed[1..]),
        _ => (false, trimmed),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(error());
    }
    let value: i64 = digits.parse().map_err(|_| error())?;
    let value = if negative { -value } else { value };
    if (0..=MAX_SOURCE_DATE_EPOCH).contains(&value) {
        Ok(value)
    } else {
        Err(error())
    }
}

// ---------------------------------------------------------------------------
// the calendar
// ---------------------------------------------------------------------------

/// The day number (days since 1970-01-01) of a proleptic Gregorian date.
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_from_march = i64::from((month + 9) % 12);
    let day_of_year = (153 * month_from_march + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The date of a day number: year, month (1–12) and day.
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_from_march + 2) / 5 + 1) as u32;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    } as u32;
    let year = year_of_era + era * 400;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

fn is_leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

// ---------------------------------------------------------------------------
// the local time zone
// ---------------------------------------------------------------------------

/// The local zone's offset from UTC at `seconds`; zero where nothing says
/// what the local zone is.
pub fn local_offset(seconds: i64) -> i64 {
    static ZONE: OnceLock<Option<Zone>> = OnceLock::new();
    ZONE.get_or_init(load_zone)
        .as_ref()
        .map_or(0, |zone| zone.offset_at(seconds))
}

/// What `TZ` names, as glibc reads it: unset is `/etc/localtime`, empty is
/// UTC, `:name` and a plain name are a TZif file (under `TZDIR` or
/// `/usr/share/zoneinfo` unless absolute), and a plain name that is no file
/// is a POSIX rule.
fn load_zone() -> Option<Zone> {
    let Some(tz) = std::env::var_os("TZ") else {
        return tzif_file(Path::new("/etc/localtime"));
    };
    let tz = tz.to_string_lossy();
    if tz.is_empty() {
        return Some(Zone::Rule(Rule::fixed(0)));
    }
    if let Some(name) = tz.strip_prefix(':') {
        return tzif_named(name);
    }
    tzif_named(&tz).or_else(|| Rule::parse(&tz).map(Zone::Rule))
}

fn tzif_named(name: &str) -> Option<Zone> {
    let path = Path::new(name);
    if path.is_absolute() {
        return tzif_file(path);
    }
    if name.split('/').any(|part| part == "..") {
        return None;
    }
    let dir = std::env::var_os("TZDIR").unwrap_or_else(|| "/usr/share/zoneinfo".into());
    tzif_file(&Path::new(&dir).join(path))
}

fn tzif_file(path: &Path) -> Option<Zone> {
    parse_tzif(&std::fs::read(path).ok()?)
}

/// A time zone: a TZif file's transitions, or a POSIX rule alone.
#[derive(Clone, Debug, PartialEq)]
enum Zone {
    /// RFC 8536's data: the moments the offset changes, the type each one
    /// changes to, the offsets of the types, and the rule for after the last.
    Tzif {
        transitions: Vec<i64>,
        types: Vec<usize>,
        offsets: Vec<i64>,
        footer: Option<Rule>,
    },
    /// A POSIX `TZ` rule.
    Rule(Rule),
}

impl Zone {
    /// The offset from UTC at `seconds`.
    fn offset_at(&self, seconds: i64) -> i64 {
        let Zone::Tzif {
            transitions,
            types,
            offsets,
            footer,
        } = self
        else {
            let Zone::Rule(rule) = self else {
                unreachable!("two variants")
            };
            return rule.offset_at(seconds);
        };
        let after_last = transitions.last().is_none_or(|last| seconds >= *last);
        if after_last && let Some(rule) = footer {
            return rule.offset_at(seconds);
        }
        // Before the first transition is the first type (RFC 8536 3.2).
        let at = transitions.partition_point(|moment| *moment <= seconds);
        let ty = match at {
            0 => 0,
            _ => types.get(at - 1).copied().unwrap_or(0),
        };
        offsets.get(ty).copied().unwrap_or(0)
    }
}

/// Parses a TZif file: version 1's 32-bit data, or the 64-bit data and the
/// POSIX rule a later version follows it with.
fn parse_tzif(bytes: &[u8]) -> Option<Zone> {
    let header = |at: usize| -> Option<[usize; 6]> {
        if bytes.get(at..at + 4)? != b"TZif" {
            return None;
        }
        let mut counts = [0usize; 6];
        for (index, count) in counts.iter_mut().enumerate() {
            let start = at + 20 + index * 4;
            *count = u32::from_be_bytes(bytes.get(start..start + 4)?.try_into().ok()?) as usize;
        }
        Some(counts)
    };
    // isutcnt, isstdcnt, leapcnt, timecnt, typecnt, charcnt.
    let [isut, isstd, leap, time, types, chars] = header(0)?;
    let version = *bytes.get(4)?;
    let v1_len = time * 5 + types * 6 + chars + leap * 8 + isstd + isut;
    let (at, width) = if version >= b'2' {
        (44 + v1_len, 8)
    } else {
        (0, 4)
    };
    let [isut, isstd, leap, time, types, chars] = header(at)?;
    let mut pos = at + 44;
    let mut transitions = Vec::with_capacity(time);
    for _ in 0..time {
        let raw = bytes.get(pos..pos + width)?;
        transitions.push(match width {
            8 => i64::from_be_bytes(raw.try_into().ok()?),
            _ => i64::from(i32::from_be_bytes(raw.try_into().ok()?)),
        });
        pos += width;
    }
    let indices: Vec<usize> = bytes
        .get(pos..pos + time)?
        .iter()
        .map(|index| usize::from(*index))
        .collect();
    pos += time;
    let mut offsets = Vec::with_capacity(types);
    for _ in 0..types {
        let raw = bytes.get(pos..pos + 4)?;
        offsets.push(i64::from(i32::from_be_bytes(raw.try_into().ok()?)));
        pos += 6;
    }
    pos += chars + leap * (width + 4) + isstd + isut;
    let footer = if width == 8 {
        bytes
            .get(pos..)
            .and_then(|rest| rest.strip_prefix(b"\n"))
            .and_then(|rest| rest.split(|b| *b == b'\n').next())
            .and_then(|rule| std::str::from_utf8(rule).ok())
            .filter(|rule| !rule.is_empty())
            .and_then(Rule::parse)
    } else {
        None
    };
    Some(Zone::Tzif {
        transitions,
        types: indices,
        offsets,
        footer,
    })
}

/// A POSIX `TZ` rule: a standard offset, and a daylight one with the two
/// moments of the year it starts and ends.
#[derive(Clone, Debug, PartialEq)]
struct Rule {
    /// The standard time's offset, east of UTC.
    standard: i64,
    /// The daylight time's offset and the transitions into and out of it.
    daylight: Option<(i64, Transition, Transition)>,
}

/// When in the year a rule changes over, in local time.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Transition {
    day: Day,
    /// Seconds after the local midnight that starts `day`.
    time: i64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Day {
    /// `Jn`: day 1–365, February 29th never counted.
    Julian(u32),
    /// `n`: day 0–365, February 29th counted.
    Zero(u32),
    /// `Mm.w.d`: weekday `d` (0 = Sunday) of week `w` (5 = the last) of month
    /// `m`.
    Month { month: u32, week: u32, weekday: u32 },
}

impl Day {
    /// The day number of this day in `year`.
    fn in_year(self, year: i64) -> i64 {
        let january = days_from_civil(year, 1, 1);
        match self {
            Day::Julian(n) => {
                let leap_skip = i64::from(is_leap(year) && n >= 60);
                january + i64::from(n) - 1 + leap_skip
            }
            Day::Zero(n) => january + i64::from(n),
            Day::Month {
                month,
                week,
                weekday,
            } => {
                let first = days_from_civil(year, month, 1);
                let first_weekday = (first + 4).rem_euclid(7);
                let offset = (i64::from(weekday) - first_weekday).rem_euclid(7);
                let mut day = 1 + offset + i64::from(week - 1) * 7;
                while day > i64::from(days_in_month(year, month)) {
                    day -= 7;
                }
                first + day - 1
            }
        }
    }
}

impl Rule {
    fn fixed(standard: i64) -> Self {
        Self {
            standard,
            daylight: None,
        }
    }

    /// The offset from UTC at `seconds`.
    fn offset_at(&self, seconds: i64) -> i64 {
        let Some((daylight, start, end)) = self.daylight else {
            return self.standard;
        };
        let (year, _, _) = civil_from_days((seconds + self.standard).div_euclid(86_400));
        // Daylight time starts at a moment told in standard time, and ends at
        // one told in daylight time.
        let starts = start.day.in_year(year) * 86_400 + start.time - self.standard;
        let ends = end.day.in_year(year) * 86_400 + end.time - daylight;
        let in_daylight = if starts < ends {
            starts <= seconds && seconds < ends
        } else {
            !(ends <= seconds && seconds < starts)
        };
        if in_daylight { daylight } else { self.standard }
    }

    /// Parses `std offset [dst [offset] [,start[/time],end[/time]]]`.
    fn parse(text: &str) -> Option<Self> {
        let mut rest = text;
        name(&mut rest)?;
        // A POSIX offset is west of UTC.
        let standard = -offset(&mut rest)?;
        if rest.is_empty() {
            return Some(Self::fixed(standard));
        }
        name(&mut rest)?;
        let daylight = if rest.is_empty() || rest.starts_with(',') {
            standard + 3600
        } else {
            -offset(&mut rest)?
        };
        // Without a rule, the United States' since 2007, as glibc has it.
        const DEFAULT_RULE: &str = ",M3.2.0,M11.1.0";
        let rule = Some(rest).filter(|rule| !rule.is_empty());
        let mut rest = rule.unwrap_or(DEFAULT_RULE).strip_prefix(',')?;
        let start = transition(&mut rest)?;
        rest = rest.strip_prefix(',')?;
        let end = transition(&mut rest)?;
        if !rest.is_empty() {
            return None;
        }
        Some(Self {
            standard,
            daylight: Some((daylight, start, end)),
        })
    }
}

/// A zone abbreviation: three or more letters, or anything in `<…>`.
fn name(rest: &mut &str) -> Option<()> {
    if let Some(quoted) = rest.strip_prefix('<') {
        let close = quoted.find('>')?;
        *rest = &quoted[close + 1..];
        return Some(());
    }
    let len = rest.bytes().take_while(|b| b.is_ascii_alphabetic()).count();
    if len < 3 {
        return None;
    }
    *rest = &rest[len..];
    Some(())
}

/// `[+-]hh[:mm[:ss]]`, in seconds.
fn offset(rest: &mut &str) -> Option<i64> {
    let sign = match rest.as_bytes().first() {
        Some(b'-') => {
            *rest = &rest[1..];
            -1
        }
        Some(b'+') => {
            *rest = &rest[1..];
            1
        }
        _ => 1,
    };
    let mut total = 0i64;
    for (index, scale) in [3600, 60, 1].into_iter().enumerate() {
        if index > 0 {
            match rest.strip_prefix(':') {
                Some(after) => *rest = after,
                None => break,
            }
        }
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        total += rest[..digits].parse::<i64>().ok()? * scale;
        *rest = &rest[digits..];
    }
    Some(sign * total)
}

/// `Jn`, `n` or `Mm.w.d`, and an optional `/time`.
fn transition(rest: &mut &str) -> Option<Transition> {
    let number = |rest: &mut &str| -> Option<u32> {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let value = rest.get(..digits)?.parse().ok()?;
        *rest = &rest[digits..];
        Some(value)
    };
    let day = if let Some(after) = rest.strip_prefix('J') {
        *rest = after;
        let n = number(rest)?;
        (1..=365).contains(&n).then_some(Day::Julian(n))?
    } else if let Some(after) = rest.strip_prefix('M') {
        *rest = after;
        let month = number(rest)?;
        *rest = rest.strip_prefix('.')?;
        let week = number(rest)?;
        *rest = rest.strip_prefix('.')?;
        let weekday = number(rest)?;
        let valid = (1..=12).contains(&month) && (1..=5).contains(&week) && weekday <= 6;
        valid.then_some(Day::Month {
            month,
            week,
            weekday,
        })?
    } else {
        let n = number(rest)?;
        (n <= 365).then_some(Day::Zero(n))?
    };
    let time = match rest.strip_prefix('/') {
        Some(after) => {
            *rest = after;
            offset(rest)?
        }
        None => 7200,
    };
    Some(Transition { day, time })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_calendar_round_trips() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(days_from_civil(2000, 2, 29), 11_016);
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        for days in (-800_000..3_000_000).step_by(997) {
            let (year, month, day) = civil_from_days(days);
            assert_eq!(days_from_civil(year, month, day), days);
        }
        assert_eq!(
            civil_from_days(MAX_SOURCE_DATE_EPOCH.div_euclid(86_400)),
            (9999, 12, 31)
        );
    }

    #[test]
    fn the_strings_are_gccs() {
        let epoch = LocalTime::utc(0);
        assert_eq!(epoch.date(), "Jan  1 1970");
        assert_eq!(epoch.time(), "00:00:00");
        assert_eq!(epoch.timestamp(), "Thu Jan  1 00:00:00 1970");
        let t = LocalTime::utc(1_700_000_000);
        assert_eq!(t.date(), "Nov 14 2023");
        assert_eq!(t.time(), "22:13:20");
        let last = LocalTime::utc(MAX_SOURCE_DATE_EPOCH);
        assert_eq!(last.date(), "Dec 31 9999");
        assert_eq!(last.time(), "23:59:59");
        // Told in JST, nine hours east.
        let tokyo = LocalTime {
            seconds: 1_791_449_135,
            utc_offset: 9 * 3600,
        };
        assert_eq!(tokyo.timestamp(), "Thu Oct  8 17:45:35 2026");
        assert_eq!(tokyo.date(), "Oct  8 2026");
    }

    #[test]
    fn source_date_epoch_is_read_as_gcc_reads_it() {
        assert_eq!(source_date_epoch("0"), Ok(0));
        assert_eq!(source_date_epoch(" 5"), Ok(5));
        assert_eq!(source_date_epoch("+7"), Ok(7));
        assert_eq!(source_date_epoch("253402300799"), Ok(MAX_SOURCE_DATE_EPOCH));
        let bad_ones = ["", "-1", "abc", "5x", "1e3", "253402300800"];
        for bad in bad_ones.into_iter().chain(["99999999999999999999"]) {
            let error = source_date_epoch(bad).unwrap_err();
            assert!(error.contains("non-negative integer"), "{bad:?}: {error}");
        }
    }

    #[test]
    fn a_posix_rule_says_when_daylight_time_is() {
        let new_york = Rule::parse("EST5EDT,M3.2.0,M11.1.0").unwrap();
        let at = |y: i64, m: u32, d: u32, h: i64| days_from_civil(y, m, d) * 86_400 + h * 3600;
        assert_eq!(new_york.offset_at(at(2026, 1, 15, 12)), -5 * 3600);
        assert_eq!(new_york.offset_at(at(2026, 7, 15, 12)), -4 * 3600);
        // 2026-03-08 02:00 EST is 07:00 UTC; 2026-11-01 02:00 EDT is 06:00.
        assert_eq!(new_york.offset_at(at(2026, 3, 8, 7) - 1), -5 * 3600);
        assert_eq!(new_york.offset_at(at(2026, 3, 8, 7)), -4 * 3600);
        assert_eq!(new_york.offset_at(at(2026, 11, 1, 6) - 1), -4 * 3600);
        assert_eq!(new_york.offset_at(at(2026, 11, 1, 6)), -5 * 3600);
        // The southern hemisphere's daylight time spans the new year.
        let sydney = Rule::parse("AEST-10AEDT,M10.1.0,M4.1.0/3").unwrap();
        assert_eq!(sydney.offset_at(at(2026, 1, 15, 0)), 11 * 3600);
        assert_eq!(sydney.offset_at(at(2026, 7, 15, 0)), 10 * 3600);
        assert_eq!(Rule::parse("JST-9").unwrap().offset_at(0), 9 * 3600);
        assert_eq!(Rule::parse("<+0530>-5:30").unwrap().offset_at(0), 19_800);
        assert!(Rule::parse("X").is_none());
    }

    /// A TZif file of the shape `zic` writes: version 2, a transition in
    /// each set of data, and the rule after the last.
    #[test]
    fn a_tzif_file_and_its_rule() {
        let mut file = Vec::new();
        let block = |file: &mut Vec<u8>, width: usize| {
            file.extend_from_slice(b"TZif2");
            file.extend_from_slice(&[0; 15]);
            for count in [0u32, 0, 0, 1, 2, 8] {
                file.extend_from_slice(&count.to_be_bytes());
            }
            // One transition, at 1000, to type 1.
            match width {
                8 => file.extend_from_slice(&1000i64.to_be_bytes()),
                _ => file.extend_from_slice(&1000i32.to_be_bytes()),
            }
            file.push(1);
            file.extend_from_slice(&3600i32.to_be_bytes());
            file.extend_from_slice(&[0, 0]);
            file.extend_from_slice(&7200i32.to_be_bytes());
            file.extend_from_slice(&[0, 4]);
            file.extend_from_slice(b"ONE\0TWO\0");
        };
        block(&mut file, 4);
        block(&mut file, 8);
        file.extend_from_slice(b"\nJST-9\n");
        let zone = parse_tzif(&file).unwrap();
        assert_eq!(zone.offset_at(0), 3600);
        assert_eq!(zone.offset_at(999), 3600);
        // From the last transition on, the rule.
        assert_eq!(zone.offset_at(1000), 9 * 3600);
        // A system zone, where there is one.
        if let Some(zone) = tzif_file(Path::new("/usr/share/zoneinfo/America/New_York")) {
            let at = |y: i64, m: u32, d: u32| days_from_civil(y, m, d) * 86_400 + 12 * 3600;
            assert_eq!(zone.offset_at(at(2026, 1, 15)), -5 * 3600);
            assert_eq!(zone.offset_at(at(2026, 7, 15)), -4 * 3600);
            assert_eq!(zone.offset_at(at(2050, 7, 15)), -4 * 3600);
            assert_eq!(zone.offset_at(at(1950, 1, 15)), -5 * 3600);
        }
    }
}
