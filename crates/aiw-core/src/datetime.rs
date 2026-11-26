//! Small application date/time boundary backed by Jiff. Keep the existing wire and SQLite
//! timestamp spellings (including precision and explicit offsets) during the migration.
use jiff::{
    civil,
    fmt::{rfc2822, strtime, temporal},
    tz::{AmbiguousOffset, Offset, TimeZone},
    SignedDuration, Timestamp, Zoned,
};
use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, AddAssign, Neg, Sub};
use std::time::{Duration as StdDuration, SystemTime};

#[derive(Clone, Copy, Debug)]
pub struct Local;
#[derive(Clone, Copy, Debug)]
pub struct Utc;

pub trait Zone: Copy {
    fn zone(self) -> TimeZone;
}
impl Zone for Local {
    fn zone(self) -> TimeZone {
        TimeZone::system()
    }
}
impl Zone for Utc {
    fn zone(self) -> TimeZone {
        TimeZone::UTC
    }
}
impl Zone for Offset {
    fn zone(self) -> TimeZone {
        self.to_time_zone()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DateTime<Z = Offset> {
    instant: Timestamp,
    zone: Z,
}
impl<Z: Zone> DateTime<Z> {
    pub fn instant(self) -> Timestamp {
        self.instant
    }
    pub fn zoned(self) -> Zoned {
        self.instant.to_zoned(self.zone.zone())
    }
    pub fn with_timezone<T: Zone>(self, zone: &T) -> DateTime<T> {
        DateTime { instant: self.instant, zone: *zone }
    }
    pub fn to_utc(self) -> DateTime<Utc> {
        self.with_timezone(&Utc)
    }
    pub fn timestamp(self) -> i64 {
        (self.instant.as_nanosecond().div_euclid(1_000_000_000)) as i64
    }
    pub fn timestamp_millis(self) -> i64 {
        (self.instant.as_nanosecond().div_euclid(1_000_000)) as i64
    }
    pub fn naive_local(self) -> NaiveDateTime {
        NaiveDateTime(self.zoned().datetime())
    }
    pub fn date_naive(self) -> NaiveDate {
        self.naive_local().date()
    }
    pub fn time(self) -> NaiveTime {
        self.naive_local().time()
    }
    pub fn year(self) -> i32 {
        self.date_naive().year()
    }
    pub fn month(self) -> u32 {
        self.date_naive().month()
    }
    pub fn month0(self) -> u32 {
        self.month() - 1
    }
    pub fn day(self) -> u32 {
        self.date_naive().day()
    }
    pub fn weekday(self) -> Weekday {
        self.date_naive().weekday()
    }
    pub fn hour(self) -> u32 {
        self.time().hour()
    }
    pub fn minute(self) -> u32 {
        self.time().minute()
    }
    pub fn format(self, fmt: &str) -> String {
        self.zoned().strftime(fmt).to_string()
    }
    pub fn to_rfc3339(self) -> String {
        self.to_rfc3339_opts(SecondsFormat::Auto, false)
    }
    pub fn to_rfc3339_opts(self, precision: SecondsFormat, use_z: bool) -> String {
        let zoned = self.zoned();
        let nano = zoned.subsec_nanosecond();
        let digits = match precision {
            SecondsFormat::Secs => 0,
            SecondsFormat::Millis => 3,
            SecondsFormat::Auto if nano == 0 => 0,
            SecondsFormat::Auto if nano % 1_000_000 == 0 => 3,
            SecondsFormat::Auto if nano % 1_000 == 0 => 6,
            SecondsFormat::Auto => 9,
        };
        let mut s = zoned.strftime("%Y-%m-%dT%H:%M:%S").to_string();
        if digits > 0 {
            let fraction = format!("{nano:09}");
            s.push('.');
            s.push_str(&fraction[..digits]);
        }
        if use_z && zoned.offset() == Offset::UTC {
            s.push('Z');
        } else {
            s.push_str(&zoned.strftime("%:z").to_string());
        }
        s
    }
    pub fn to_rfc2822(self) -> String {
        self.zoned().strftime("%a, %-d %b %Y %H:%M:%S %z").to_string()
    }
}
#[derive(Clone, Copy)]
pub enum SecondsFormat {
    Auto,
    Secs,
    Millis,
}
#[derive(Debug, thiserror::Error)]
pub enum TimestampParseError {
    #[error("expected an RFC 3339 timestamp with a numeric offset or Z")]
    Shape,
    #[error(transparent)]
    Value(#[from] jiff::Error),
}

impl DateTime<Offset> {
    pub fn parse_from_rfc3339(s: &str) -> Result<Self, TimestampParseError> {
        // Jiff also accepts ISO compact forms and zone annotations. Do not widen the
        // accepted input for stored expiries/API fields in this migration.
        let b = s.as_bytes();
        if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't' | b' ') || b[13] != b':' || b[16] != b':' {
            return Err(TimestampParseError::Shape);
        }
        let mut off = 19;
        if b[off] == b'.' {
            off += 1;
            off += b[off..].iter().take_while(|c| c.is_ascii_digit()).count();
            if off == 20 {
                return Err(TimestampParseError::Shape);
            }
        }
        match &b[off..] {
            [b'Z' | b'z'] => {}
            [b'+' | b'-', h1, h2, b':', m1, m2]
                if [h1, h2, m1, m2].into_iter().all(u8::is_ascii_digit) && (h1 - b'0') * 10 + (h2 - b'0') <= 23 => {}
            _ => return Err(TimestampParseError::Shape),
        }
        let parser = temporal::DateTimeParser::new();
        let pieces = parser.parse_pieces(s)?;
        let instant = parser.parse_timestamp(s)?;
        let zone = pieces.offset().map(|offset| offset.to_numeric_offset()).unwrap_or(Offset::UTC);
        Ok(Self { instant, zone })
    }
    pub fn parse_from_rfc2822(s: &str) -> Result<Self, jiff::Error> {
        let z = rfc2822::DateTimeParser::new().parse_zoned(s)?;
        Ok(Self { instant: z.timestamp(), zone: z.offset() })
    }
    pub fn parse_from_str(s: &str, fmt: &str) -> Result<Self, jiff::Error> {
        let z = strtime::parse(fmt, s)?.to_zoned()?;
        Ok(Self { instant: z.timestamp(), zone: z.offset() })
    }
}
impl Local {
    pub fn now() -> DateTime<Self> {
        DateTime { instant: Timestamp::now(), zone: Self }
    }
    #[allow(clippy::wrong_self_convention)]
    pub fn from_local_datetime(self, date: &NaiveDateTime) -> LocalResult {
        local_datetime(&self.zone(), *date)
    }
    #[allow(clippy::wrong_self_convention)]
    pub fn from_utc_datetime(self, date: &NaiveDateTime) -> DateTime<Self> {
        date.and_utc().with_timezone(&self)
    }
    pub fn with_ymd_and_hms(self, y: i32, m: u32, d: u32, h: u32, min: u32, s: u32) -> Option<DateTime<Self>> {
        let date = NaiveDate::from_ymd_opt(y, m, d)?.and_hms_opt(h, min, s)?;
        self.from_local_datetime(&date).single()
    }
}
impl Utc {
    pub fn now() -> DateTime<Self> {
        DateTime { instant: Timestamp::now(), zone: Self }
    }
}
impl TryFrom<SystemTime> for DateTime<Local> {
    type Error = jiff::Error;
    fn try_from(t: SystemTime) -> Result<Self, Self::Error> {
        Ok(Self { instant: Timestamp::try_from(t)?, zone: Local })
    }
}
impl<Z: Zone> fmt::Display for DateTime<Z> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_rfc3339())
    }
}
/// A repeated wall time can be resolved to its first occurrence; a missing time remains missing
/// so the caller retains its previous fallback instead of silently shifting it an hour.
pub struct LocalResult(AmbiguousOffset, Option<Timestamp>);
fn local_datetime(zone: &TimeZone, date: NaiveDateTime) -> LocalResult {
    let ambiguous = zone.to_ambiguous_timestamp(date.0);
    let offset = ambiguous.offset();
    let first = match offset {
        AmbiguousOffset::Gap { .. } => None,
        _ => ambiguous.earlier().ok(),
    };
    LocalResult(offset, first)
}
impl LocalResult {
    pub fn earliest(self) -> Option<DateTime<Local>> {
        self.1.map(|instant| DateTime { instant, zone: Local })
    }
    pub fn single(self) -> Option<DateTime<Local>> {
        if matches!(self.0, AmbiguousOffset::Unambiguous { .. }) {
            self.earliest()
        } else {
            None
        }
    }
}
impl<Z, T> PartialEq<DateTime<T>> for DateTime<Z> {
    fn eq(&self, rhs: &DateTime<T>) -> bool {
        self.instant == rhs.instant
    }
}
impl<Z> Eq for DateTime<Z> {}
impl<Z, T> PartialOrd<DateTime<T>> for DateTime<Z> {
    fn partial_cmp(&self, rhs: &DateTime<T>) -> Option<Ordering> {
        Some(self.instant.cmp(&rhs.instant))
    }
}
impl<Z> Ord for DateTime<Z> {
    fn cmp(&self, rhs: &Self) -> Ordering {
        self.instant.cmp(&rhs.instant)
    }
}
impl<Z> Add<Duration> for DateTime<Z> {
    type Output = Self;
    fn add(self, rhs: Duration) -> Self {
        Self { instant: self.instant + rhs.0, zone: self.zone }
    }
}
impl<Z> Sub<Duration> for DateTime<Z> {
    type Output = Self;
    fn sub(self, rhs: Duration) -> Self {
        Self { instant: self.instant - rhs.0, zone: self.zone }
    }
}
impl<Z, T> Sub<DateTime<T>> for DateTime<Z> {
    type Output = Duration;
    fn sub(self, rhs: DateTime<T>) -> Duration {
        Duration(self.instant.duration_since(rhs.instant))
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Duration(SignedDuration);
impl Duration {
    pub fn zero() -> Self {
        Self(SignedDuration::ZERO)
    }
    pub fn milliseconds(n: i64) -> Self {
        Self(SignedDuration::from_millis(n))
    }
    pub fn seconds(n: i64) -> Self {
        Self(SignedDuration::from_secs(n))
    }
    pub fn minutes(n: i64) -> Self {
        Self(SignedDuration::from_mins(n))
    }
    pub fn hours(n: i64) -> Self {
        Self(SignedDuration::from_hours(n))
    }
    pub fn days(n: i64) -> Self {
        Self::hours(n.checked_mul(24).expect("days in signed duration range"))
    }
    pub fn weeks(n: i64) -> Self {
        Self::days(n.checked_mul(7).expect("weeks in signed duration range"))
    }
    pub fn num_seconds(self) -> i64 {
        self.0.as_secs()
    }
    pub fn num_days(self) -> i64 {
        self.0.as_secs() / 86_400
    }
    pub fn from_std(d: StdDuration) -> Result<Self, jiff::Error> {
        SignedDuration::try_from(d).map(Self)
    }
    pub fn to_std(self) -> Result<StdDuration, jiff::Error> {
        StdDuration::try_from(self.0)
    }
}
impl Neg for Duration {
    type Output = Self;
    fn neg(self) -> Self {
        Self(-self.0)
    }
}
impl AddAssign for Duration {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct NaiveDate(civil::Date);
impl NaiveDate {
    pub fn from_ymd_opt(y: i32, m: u32, d: u32) -> Option<Self> {
        civil::Date::new(y.try_into().ok()?, m.try_into().ok()?, d.try_into().ok()?).ok().map(Self)
    }
    pub fn parse_from_str(s: &str, fmt: &str) -> Result<Self, jiff::Error> {
        civil::Date::strptime(fmt, s).map(Self)
    }
    pub fn and_time(self, t: NaiveTime) -> NaiveDateTime {
        NaiveDateTime(self.0.to_datetime(t.0))
    }
    pub fn and_hms_opt(self, h: u32, m: u32, s: u32) -> Option<NaiveDateTime> {
        Some(self.and_time(NaiveTime::from_hms_opt(h, m, s)?))
    }
    pub fn year(self) -> i32 {
        self.0.year().into()
    }
    pub fn month(self) -> u32 {
        self.0.month() as u32
    }
    pub fn month0(self) -> u32 {
        self.month() - 1
    }
    pub fn day(self) -> u32 {
        self.0.day() as u32
    }
    pub fn weekday(self) -> Weekday {
        Weekday(self.0.weekday())
    }
    pub fn format(self, fmt: &str) -> String {
        self.0.strftime(fmt).to_string()
    }
}
impl Add<Duration> for NaiveDate {
    type Output = Self;
    fn add(self, rhs: Duration) -> Self {
        Self(self.0 + rhs.0)
    }
}
impl Sub<Duration> for NaiveDate {
    type Output = Self;
    fn sub(self, rhs: Duration) -> Self {
        Self(self.0 - rhs.0)
    }
}
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct NaiveDateTime(civil::DateTime);
impl NaiveDateTime {
    pub fn parse_from_str(s: &str, fmt: &str) -> Result<Self, jiff::Error> {
        civil::DateTime::strptime(fmt, s).map(Self)
    }
    pub fn date(self) -> NaiveDate {
        NaiveDate(self.0.date())
    }
    pub fn time(self) -> NaiveTime {
        NaiveTime(self.0.time())
    }
    pub fn weekday(self) -> Weekday {
        self.date().weekday()
    }
    pub fn and_utc(self) -> DateTime<Utc> {
        DateTime { instant: self.0.to_zoned(TimeZone::UTC).expect("civil date in UTC").timestamp(), zone: Utc }
    }
}
impl Add<Duration> for NaiveDateTime {
    type Output = Self;
    fn add(self, rhs: Duration) -> Self {
        Self(self.0 + rhs.0)
    }
}
impl Sub<Duration> for NaiveDateTime {
    type Output = Self;
    fn sub(self, rhs: Duration) -> Self {
        Self(self.0 - rhs.0)
    }
}
impl Sub for NaiveDateTime {
    type Output = Duration;
    fn sub(self, rhs: Self) -> Duration {
        Duration(self.0.duration_since(rhs.0))
    }
}
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct NaiveTime(civil::Time);
impl NaiveTime {
    pub fn from_hms_opt(h: u32, m: u32, s: u32) -> Option<Self> {
        civil::Time::new(h.try_into().ok()?, m.try_into().ok()?, s.try_into().ok()?, 0).ok().map(Self)
    }
    pub fn hour(self) -> u32 {
        self.0.hour() as u32
    }
    pub fn minute(self) -> u32 {
        self.0.minute() as u32
    }
    pub fn format(self, fmt: &str) -> String {
        self.0.strftime(fmt).to_string()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Weekday(civil::Weekday);
#[allow(non_upper_case_globals)]
impl Weekday {
    pub const Mon: Self = Self(civil::Weekday::Monday);
    pub const Tue: Self = Self(civil::Weekday::Tuesday);
    pub const Wed: Self = Self(civil::Weekday::Wednesday);
    pub const Thu: Self = Self(civil::Weekday::Thursday);
    pub const Fri: Self = Self(civil::Weekday::Friday);
    pub const Sat: Self = Self(civil::Weekday::Saturday);
    pub const Sun: Self = Self(civil::Weekday::Sunday);
    pub fn num_days_from_monday(self) -> u32 {
        self.0.to_monday_zero_offset() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_wire_timestamps_keep_their_spelling_and_instant() {
        for s in [
            "2026-11-26T14:23:59+01:00",
            "2026-11-26T14:23:59.123+05:30",
            "2026-11-26T14:23:59.123400-03:30",
            "2026-11-26T14:23:59.123456700+00:00",
        ] {
            assert_eq!(DateTime::parse_from_rfc3339(s).unwrap().to_rfc3339(), s);
        }
        let d = DateTime::parse_from_rfc3339("2026-11-26T14:23:59.009800+01:00").unwrap().to_utc();
        assert_eq!(d.to_rfc3339_opts(SecondsFormat::Millis, true), "2026-11-26T13:23:59.009Z");
        assert_eq!(d.to_rfc3339_opts(SecondsFormat::Secs, true), "2026-11-26T13:23:59Z");
        let same = DateTime::parse_from_rfc3339("2026-11-26T13:23:59.009800Z").unwrap();
        assert_eq!(same, d);
        assert!(DateTime::parse_from_rfc3339("not a timestamp").is_err());
        for invalid in [
            "2026-11-26T13:23:59",
            "2026-11-26T13:23:59+0100",
            "2026-11-26T13:23:59+24:00",
            "2026-11-26T13:23:59Z[UTC]",
            "2026-11-26T13:23:59.Z",
        ] {
            assert!(DateTime::parse_from_rfc3339(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn unix_epoch_and_expiry_boundaries_are_unchanged() {
        let epoch = DateTime::parse_from_rfc3339("1970-01-01T00:00:00Z").unwrap();
        let before = DateTime::parse_from_rfc3339("1969-12-31T23:59:59.999999999Z").unwrap();
        assert_eq!((before.timestamp(), before.timestamp_millis()), (-1, -1));
        assert!(before < epoch);
        assert!((before - epoch).to_std().is_err());
        assert_eq!(((epoch + Duration::seconds(300)) - epoch).num_seconds(), 300);
    }

    #[test]
    fn mail_and_imap_formats_keep_offsets_and_day_of_month() {
        let from_mail = DateTime::parse_from_rfc2822("Mon, 5 Mar 2029 09:00:00 +0130").unwrap();
        let from_imap = DateTime::parse_from_str("05-Mar-2029 09:00:00 +0130", "%d-%b-%Y %H:%M:%S %z").unwrap();
        assert_eq!(from_mail, from_imap);
        assert_eq!(from_mail.to_rfc2822(), "Mon, 5 Mar 2029 09:00:00 +0130");
        assert_eq!(from_mail.to_utc().to_rfc3339(), "2029-03-05T07:30:00+00:00");
    }

    #[test]
    fn local_gap_is_missing_and_fold_chooses_the_earlier_instant() {
        let rome = TimeZone::get("Europe/Rome").unwrap();
        let gap = NaiveDateTime(civil::date(2026, 3, 29).at(2, 30, 0, 0));
        assert!(local_datetime(&rome, gap).earliest().is_none());
        let fold = NaiveDateTime(civil::date(2026, 10, 25).at(2, 30, 0, 0));
        assert!(local_datetime(&rome, fold).single().is_none());
        assert_eq!(local_datetime(&rome, fold).earliest().unwrap().to_utc().to_rfc3339(), "2026-10-25T00:30:00+00:00");
    }
}
