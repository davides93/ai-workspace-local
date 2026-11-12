//! A compact iCalendar (RFC 5545) reader: just what a "what is on today" question needs.
//!
//! Handled: `VEVENT` with `DTSTART` / `DTEND` / `DURATION` / `SUMMARY` / `LOCATION`, all-day events
//! (`VALUE=DATE`), UTC (`…Z`), `TZID` zones from a small built-in table (common European, US and a few
//! Asian zones, with their daylight-saving rules; an unknown `TZID` is read as the computer's own time),
//! floating times, `EXDATE`, `RECURRENCE-ID` overrides, `STATUS:CANCELLED`, and the recurrence rule
//! subset `FREQ` DAILY / WEEKLY / MONTHLY / YEARLY with `INTERVAL`, `COUNT`, `UNTIL` and (weekly) `BYDAY`.
//! A rule using anything else (`BYSETPOS`, `BYMONTHDAY`, ordinals in `BYDAY`…) is not guessed at: the
//! first occurrence is shown and the rest is left out.

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Weekday};
use std::collections::HashSet;

/// One occurrence of an event, in the computer's local time.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub summary: String,
    pub location: String,
    pub start: DateTime<Local>,
    /// Exclusive for all-day events (midnight of the day after the last day).
    pub end: DateTime<Local>,
    pub all_day: bool,
}

// ------------------------------------------------------------------------------------------ zones

#[derive(Debug, Clone, Copy, PartialEq)]
enum Dst {
    None,
    Eu,
    Us,
}

#[derive(Debug, PartialEq)]
struct Tz {
    std_min: i32,
    dst: Dst,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Zone {
    /// The computer's own zone (floating times and zones this reader does not know).
    Local,
    Utc,
    Named(&'static Tz),
}

const fn tz(std_min: i32, dst: Dst) -> Tz {
    Tz { std_min, dst }
}
static CET: Tz = tz(60, Dst::Eu);
static WET: Tz = tz(0, Dst::Eu);
static EET: Tz = tz(120, Dst::Eu);
static MSK: Tz = tz(180, Dst::None);
static US_E: Tz = tz(-300, Dst::Us);
static US_C: Tz = tz(-360, Dst::Us);
static US_M: Tz = tz(-420, Dst::Us);
static US_P: Tz = tz(-480, Dst::Us);
static MST_NO_DST: Tz = tz(-420, Dst::None);
static JST: Tz = tz(540, Dst::None);
static CST_CN: Tz = tz(480, Dst::None);
static IST: Tz = tz(330, Dst::None);
static GST: Tz = tz(240, Dst::None);

fn zone_for(tzid: &str) -> Zone {
    let id = tzid.trim().trim_matches('"');
    // `/mozilla.org/20050126_1/Europe/Rome` → `Europe/Rome`
    let parts: Vec<&str> = id.split('/').filter(|p| !p.is_empty()).collect();
    let id = if parts.len() >= 2 { format!("{}/{}", parts[parts.len() - 2], parts[parts.len() - 1]) } else { id.to_string() };
    match id.as_str() {
        "UTC" | "Etc/UTC" | "GMT" | "Etc/GMT" | "Z" => Zone::Utc,
        "Europe/Rome"
        | "Europe/Berlin"
        | "Europe/Paris"
        | "Europe/Madrid"
        | "Europe/Amsterdam"
        | "Europe/Brussels"
        | "Europe/Vienna"
        | "Europe/Zurich"
        | "Europe/Stockholm"
        | "Europe/Oslo"
        | "Europe/Copenhagen"
        | "Europe/Prague"
        | "Europe/Warsaw"
        | "Europe/Budapest"
        | "Europe/Belgrade"
        | "Europe/Luxembourg"
        | "W. Europe Standard Time"
        | "Romance Standard Time"
        | "Central Europe Standard Time"
        | "Central European Standard Time" => Zone::Named(&CET),
        "Europe/London" | "Europe/Lisbon" | "Europe/Dublin" | "GMT Standard Time" => Zone::Named(&WET),
        "Europe/Athens" | "Europe/Helsinki" | "Europe/Kiev" | "Europe/Kyiv" | "Europe/Bucharest" | "Europe/Sofia" | "FLE Standard Time"
        | "GTB Standard Time" => Zone::Named(&EET),
        "Europe/Moscow" | "Europe/Istanbul" => Zone::Named(&MSK),
        "America/New_York" | "America/Toronto" | "Eastern Standard Time" => Zone::Named(&US_E),
        "America/Chicago" | "Central Standard Time" => Zone::Named(&US_C),
        "America/Denver" | "Mountain Standard Time" => Zone::Named(&US_M),
        "America/Los_Angeles" | "America/Vancouver" | "Pacific Standard Time" => Zone::Named(&US_P),
        "America/Phoenix" => Zone::Named(&MST_NO_DST),
        "Asia/Tokyo" | "Tokyo Standard Time" => Zone::Named(&JST),
        "Asia/Shanghai" | "Asia/Hong_Kong" | "Asia/Singapore" | "China Standard Time" => Zone::Named(&CST_CN),
        "Asia/Kolkata" | "Asia/Calcutta" | "India Standard Time" => Zone::Named(&IST),
        "Asia/Dubai" | "Arabian Standard Time" => Zone::Named(&GST),
        _ => Zone::Local,
    }
}

fn nth_weekday(year: i32, month: u32, wd: Weekday, n: u32) -> NaiveDate {
    let first = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
    let delta = (7 + wd.num_days_from_monday() as i64 - first.weekday().num_days_from_monday() as i64) % 7;
    first + Duration::days(delta + 7 * (n as i64 - 1))
}

fn last_weekday(year: i32, month: u32, wd: Weekday) -> NaiveDate {
    let next = if month == 12 { NaiveDate::from_ymd_opt(year + 1, 1, 1) } else { NaiveDate::from_ymd_opt(year, month + 1, 1) }.unwrap();
    let last = next - Duration::days(1);
    let back = (7 + last.weekday().num_days_from_monday() as i64 - wd.num_days_from_monday() as i64) % 7;
    last - Duration::days(back)
}

impl Tz {
    /// Daylight-saving time in force at the UTC instant `utc`?
    fn dst_at_utc(&self, utc: NaiveDateTime) -> bool {
        let y = utc.date().year();
        let at = |d: NaiveDate, h: u32| d.and_time(NaiveTime::from_hms_opt(h, 0, 0).unwrap());
        match self.dst {
            Dst::None => false,
            Dst::Eu => utc >= at(last_weekday(y, 3, Weekday::Sun), 1) && utc < at(last_weekday(y, 10, Weekday::Sun), 1),
            Dst::Us => {
                let start = at(nth_weekday(y, 3, Weekday::Sun, 2), 2) - Duration::minutes(self.std_min as i64);
                let end = at(nth_weekday(y, 11, Weekday::Sun, 1), 2) - Duration::minutes(self.std_min as i64 + 60);
                utc >= start && utc < end
            }
        }
    }

    fn to_utc(&self, local: NaiveDateTime) -> NaiveDateTime {
        let as_std = local - Duration::minutes(self.std_min as i64);
        if self.dst_at_utc(as_std) {
            as_std - Duration::minutes(60)
        } else {
            as_std
        }
    }
}

impl Zone {
    fn to_local(self, n: NaiveDateTime) -> DateTime<Local> {
        match self {
            Zone::Local => Local.from_local_datetime(&n).earliest().unwrap_or_else(|| Local.from_utc_datetime(&n)),
            Zone::Utc => Local.from_utc_datetime(&n),
            Zone::Named(t) => Local.from_utc_datetime(&t.to_utc(n)),
        }
    }
}

// ------------------------------------------------------------------------------------------ lines

#[derive(Debug)]
struct Prop {
    name: String,
    params: Vec<(String, String)>,
    value: String,
}

impl Prop {
    fn param(&self, k: &str) -> Option<&str> {
        self.params.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
}

/// Joins folded lines (a line starting with a space or tab continues the previous one).
fn unfold(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim_end_matches('\r');
        if (line.starts_with(' ') || line.starts_with('\t')) && !out.is_empty() {
            out.last_mut().unwrap().push_str(&line[1..]);
        } else if !line.is_empty() {
            out.push(line.to_string());
        }
    }
    out
}

fn parse_prop(line: &str) -> Option<Prop> {
    // The first `:` outside double quotes ends the name and parameters.
    let mut in_q = false;
    let mut colon = None;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_q = !in_q,
            ':' if !in_q => {
                colon = Some(i);
                break;
            }
            _ => {}
        }
    }
    let colon = colon?;
    let (head, value) = (&line[..colon], &line[colon + 1..]);
    let mut parts = Vec::new();
    let (mut cur, mut q) = (String::new(), false);
    for c in head.chars() {
        match c {
            '"' => q = !q,
            ';' if !q => parts.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    parts.push(cur);
    let name = parts.remove(0).to_ascii_uppercase();
    let params =
        parts.into_iter().filter_map(|p| p.split_once('=').map(|(k, v)| (k.trim().to_ascii_uppercase(), v.trim().to_string()))).collect();
    Some(Prop { name, params, value: value.to_string() })
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(o) => out.push(o),
            None => {}
        }
    }
    out
}

// ------------------------------------------------------------------------------------------ values

#[derive(Debug, Clone, Copy, PartialEq)]
enum Dt {
    Date(NaiveDate),
    Time(NaiveDateTime, Zone),
}

impl Dt {
    fn naive(self) -> NaiveDateTime {
        match self {
            Dt::Date(d) => d.and_hms_opt(0, 0, 0).unwrap(),
            Dt::Time(n, _) => n,
        }
    }
    fn zone(self) -> Zone {
        match self {
            Dt::Date(_) => Zone::Local,
            Dt::Time(_, z) => z,
        }
    }
    fn instant(self) -> DateTime<Local> {
        self.zone().to_local(self.naive())
    }
}

fn parse_dt(p: &Prop) -> Option<Dt> {
    parse_dt_value(p.value.trim(), p.param("VALUE") == Some("DATE"), p.param("TZID"))
}

fn parse_dt_value(v: &str, date_only: bool, tzid: Option<&str>) -> Option<Dt> {
    if date_only || v.len() == 8 {
        return NaiveDate::parse_from_str(v, "%Y%m%d").ok().map(Dt::Date);
    }
    let (body, utc) = match v.strip_suffix('Z') {
        Some(b) => (b, true),
        None => (v, false),
    };
    let n = NaiveDateTime::parse_from_str(body, "%Y%m%dT%H%M%S").ok()?;
    let zone = if utc { Zone::Utc } else { tzid.map(zone_for).unwrap_or(Zone::Local) };
    Some(Dt::Time(n, zone))
}

fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    let (neg, s) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let s = s.strip_prefix('P')?;
    let mut total = Duration::zero();
    let mut num = String::new();
    for c in s.chars() {
        match c {
            '0'..='9' => num.push(c),
            'T' => {}
            'W' | 'D' | 'H' | 'M' | 'S' => {
                let n: i64 = num.parse().ok()?;
                num.clear();
                total += match c {
                    'W' => Duration::weeks(n),
                    'D' => Duration::days(n),
                    'H' => Duration::hours(n),
                    'M' => Duration::minutes(n),
                    _ => Duration::seconds(n),
                };
            }
            _ => return None,
        }
    }
    Some(if neg { -total } else { total })
}

// ------------------------------------------------------------------------------------------ rules

#[derive(Debug, Clone, Copy, PartialEq)]
enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Debug)]
struct Rule {
    freq: Freq,
    interval: u32,
    count: Option<u32>,
    until: Option<DateTime<Local>>,
    byday: Vec<Weekday>,
}

/// `None` when the rule uses something this reader does not do (the caller then shows the first occurrence only).
fn parse_rule(s: &str) -> Option<Rule> {
    let mut rule = Rule { freq: Freq::Daily, interval: 1, count: None, until: None, byday: vec![] };
    let mut has_freq = false;
    for part in s.split(';') {
        let Some((k, v)) = part.split_once('=') else { continue };
        match k.trim().to_ascii_uppercase().as_str() {
            "FREQ" => {
                rule.freq = match v.trim().to_ascii_uppercase().as_str() {
                    "DAILY" => Freq::Daily,
                    "WEEKLY" => Freq::Weekly,
                    "MONTHLY" => Freq::Monthly,
                    "YEARLY" => Freq::Yearly,
                    _ => return None,
                };
                has_freq = true;
            }
            "INTERVAL" => rule.interval = v.trim().parse::<u32>().ok()?.max(1),
            "COUNT" => rule.count = Some(v.trim().parse().ok()?),
            "UNTIL" => {
                rule.until = Some(match parse_dt_value(v.trim(), false, None)? {
                    Dt::Date(d) => Local.from_local_datetime(&d.and_hms_opt(23, 59, 59)?).earliest()?,
                    t => t.instant(),
                })
            }
            "BYDAY" => {
                if rule.freq != Freq::Weekly {
                    return None;
                }
                for d in v.split(',') {
                    rule.byday.push(match d.trim().to_ascii_uppercase().as_str() {
                        "MO" => Weekday::Mon,
                        "TU" => Weekday::Tue,
                        "WE" => Weekday::Wed,
                        "TH" => Weekday::Thu,
                        "FR" => Weekday::Fri,
                        "SA" => Weekday::Sat,
                        "SU" => Weekday::Sun,
                        _ => return None, // ordinals such as 2MO
                    });
                }
            }
            "WKST" => {}
            _ => return None,
        }
    }
    has_freq.then_some(rule)
}

fn add_months(d: NaiveDate, months: i64) -> Option<NaiveDate> {
    let idx = d.year() as i64 * 12 + d.month0() as i64 + months;
    NaiveDate::from_ymd_opt((idx.div_euclid(12)) as i32, (idx.rem_euclid(12)) as u32 + 1, d.day())
}

struct Gen {
    out: Vec<NaiveDateTime>,
    zone: Zone,
    count: Option<u32>,
    until: Option<DateTime<Local>>,
    limit: NaiveDateTime,
}

impl Gen {
    /// Adds an occurrence; `false` when the series is over (or past the window).
    fn add(&mut self, n: NaiveDateTime) -> bool {
        if n > self.limit || self.until.is_some_and(|u| self.zone.to_local(n) > u) {
            return false;
        }
        self.out.push(n);
        !self.count.is_some_and(|c| self.out.len() as u32 >= c)
    }
}

const MAX_STEPS: i64 = 40_000;

fn occurrences(start: NaiveDateTime, rule: &Rule, zone: Zone, limit: NaiveDateTime) -> Vec<NaiveDateTime> {
    let mut g = Gen { out: vec![], zone, count: rule.count, until: rule.until, limit };
    let step = rule.interval as i64;
    match rule.freq {
        Freq::Daily => {
            for k in 0..MAX_STEPS {
                if !g.add(start + Duration::days(k * step)) {
                    break;
                }
            }
        }
        Freq::Weekly => {
            let mut days: Vec<Weekday> = if rule.byday.is_empty() { vec![start.weekday()] } else { rule.byday.clone() };
            days.sort_by_key(|d| d.num_days_from_monday());
            days.dedup();
            let week0 = start.date() - Duration::days(start.weekday().num_days_from_monday() as i64);
            'weeks: for w in 0..MAX_STEPS {
                for d in &days {
                    let date = week0 + Duration::days(w * step * 7 + d.num_days_from_monday() as i64);
                    let n = date.and_time(start.time());
                    if n < start {
                        continue;
                    }
                    if !g.add(n) {
                        break 'weeks;
                    }
                }
            }
        }
        Freq::Monthly | Freq::Yearly => {
            let per = if rule.freq == Freq::Monthly { 1 } else { 12 };
            for k in 0..MAX_STEPS {
                // A month without that day (the 31st) or a missing 29 February is skipped, not moved.
                let Some(date) = add_months(start.date(), k * step * per) else {
                    if k * step * per > 12 * 400 {
                        break;
                    }
                    continue;
                };
                if !g.add(date.and_time(start.time())) {
                    break;
                }
            }
        }
    }
    g.out
}

// ------------------------------------------------------------------------------------------ events

#[derive(Debug, Default)]
struct Raw {
    uid: String,
    summary: String,
    location: String,
    start: Option<Dt>,
    end: Option<Dt>,
    duration: Option<Duration>,
    rrule: Option<String>,
    exdates: Vec<Dt>,
    recurrence_id: Option<Dt>,
    cancelled: bool,
}

fn parse_raw(text: &str) -> Vec<Raw> {
    let mut out = Vec::new();
    let mut cur: Option<Raw> = None;
    let mut nested = 0u32;
    for line in unfold(text) {
        let Some(p) = parse_prop(&line) else { continue };
        match (p.name.as_str(), p.value.trim().to_ascii_uppercase().as_str()) {
            ("BEGIN", "VEVENT") if cur.is_none() => {
                cur = Some(Raw::default());
                continue;
            }
            ("BEGIN", _) if cur.is_some() => {
                nested += 1; // VALARM and friends
                continue;
            }
            ("END", "VEVENT") if nested == 0 => {
                if let Some(r) = cur.take() {
                    out.push(r);
                }
                continue;
            }
            ("END", _) if cur.is_some() => {
                nested = nested.saturating_sub(1);
                continue;
            }
            _ => {}
        }
        let Some(r) = cur.as_mut() else { continue };
        if nested > 0 {
            continue;
        }
        match p.name.as_str() {
            "UID" => r.uid = p.value.trim().to_string(),
            "SUMMARY" => r.summary = unescape(&p.value).trim().to_string(),
            "LOCATION" => r.location = unescape(&p.value).trim().to_string(),
            "DTSTART" => r.start = parse_dt(&p),
            "DTEND" => r.end = parse_dt(&p),
            "DURATION" => r.duration = parse_duration(&p.value),
            "RRULE" => r.rrule = Some(p.value.trim().to_string()),
            "EXDATE" => {
                let date_only = p.param("VALUE") == Some("DATE");
                r.exdates.extend(p.value.split(',').filter_map(|v| parse_dt_value(v.trim(), date_only, p.param("TZID"))));
            }
            "RECURRENCE-ID" => r.recurrence_id = parse_dt(&p),
            "STATUS" => r.cancelled = p.value.trim().eq_ignore_ascii_case("CANCELLED"),
            _ => {}
        }
    }
    out
}

fn same_instant(a: Dt, b: DateTime<Local>) -> bool {
    match a {
        Dt::Date(d) => b.date_naive() == d,
        t => t.instant() == b,
    }
}

/// Occurrences of every event of `text` that overlap `[from, to)`, by start time.
pub fn events_between(text: &str, from: DateTime<Local>, to: DateTime<Local>) -> Vec<Event> {
    let raws = parse_raw(text);
    let overrides: HashSet<(String, i64)> =
        raws.iter().filter_map(|r| r.recurrence_id.map(|d| (r.uid.clone(), d.instant().timestamp()))).collect();
    let limit = to.naive_local() + Duration::days(2);
    let mut out = Vec::new();
    for r in raws.iter().filter(|r| !r.cancelled) {
        let Some(start) = r.start else { continue };
        let all_day = matches!(start, Dt::Date(_));
        let zone = start.zone();
        // Length of one occurrence.
        let len = match (r.end, r.duration) {
            (Some(e), _) if e.instant() >= start.instant() => e.instant() - start.instant(),
            (_, Some(d)) => d,
            _ if all_day => Duration::days(1),
            _ => Duration::zero(),
        };
        let starts: Vec<NaiveDateTime> = match r.rrule.as_deref().and_then(parse_rule) {
            Some(rule) if r.recurrence_id.is_none() => occurrences(start.naive(), &rule, zone, limit),
            _ => vec![start.naive()],
        };
        for n in starts {
            let s = zone.to_local(n);
            if r.recurrence_id.is_none()
                && (overrides.contains(&(r.uid.clone(), s.timestamp())) || r.exdates.iter().any(|x| same_instant(*x, s)))
            {
                continue;
            }
            let e = if all_day {
                let days = len.num_days().max(1);
                zone.to_local(n + Duration::days(days))
            } else {
                s + len
            };
            let overlaps = if e > s { e > from && s < to } else { s >= from && s < to };
            if overlaps {
                out.push(Event { summary: r.summary.clone(), location: r.location.clone(), start: s, end: e, all_day });
            }
        }
    }
    out.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.summary.cmp(&b.summary)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32, h: u32, mi: u32) -> DateTime<Local> {
        Local.from_local_datetime(&NaiveDate::from_ymd_opt(y, m, day).unwrap().and_hms_opt(h, mi, 0).unwrap()).earliest().unwrap()
    }

    fn cal(body: &str) -> String {
        format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n{body}END:VCALENDAR\r\n")
    }

    #[test]
    fn a_simple_event_with_escapes_and_folded_lines() {
        let ics = cal("BEGIN:VEVENT\r\nUID:1\r\nDTSTART:20300610T093000\r\nDTEND:20300610T103000\r\nSUMMARY:Riunione\\, con il team\r\nLOCATION:Sala\r\n \" Rossa\"\r\nEND:VEVENT\r\n");
        let ev = events_between(&ics, d(2030, 6, 10, 0, 0), d(2030, 6, 11, 0, 0));
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].summary, "Riunione, con il team");
        assert_eq!(ev[0].location, "Sala\" Rossa\"");
        assert_eq!((ev[0].start, ev[0].end), (d(2030, 6, 10, 9, 30), d(2030, 6, 10, 10, 30)));
        assert!(!ev[0].all_day);
        assert!(events_between(&ics, d(2030, 6, 11, 0, 0), d(2030, 6, 12, 0, 0)).is_empty());
    }

    #[test]
    fn all_day_events_end_the_day_after_and_multi_day_ones_overlap_each_day() {
        let ics = cal("BEGIN:VEVENT\r\nUID:a\r\nDTSTART;VALUE=DATE:20300610\r\nDTEND;VALUE=DATE:20300613\r\nSUMMARY:Gita\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:b\r\nDTSTART;VALUE=DATE:20300611\r\nSUMMARY:Compleanno\r\nEND:VEVENT\r\n");
        let day11 = events_between(&ics, d(2030, 6, 11, 0, 0), d(2030, 6, 12, 0, 0));
        assert_eq!(day11.iter().map(|e| e.summary.as_str()).collect::<Vec<_>>(), ["Gita", "Compleanno"]);
        assert!(day11.iter().all(|e| e.all_day));
        assert!(events_between(&ics, d(2030, 6, 13, 0, 0), d(2030, 6, 14, 0, 0)).is_empty(), "DTEND of an all-day event is exclusive");
        assert_eq!(events_between(&ics, d(2030, 6, 12, 0, 0), d(2030, 6, 13, 0, 0)).len(), 1);
    }

    #[test]
    fn utc_and_named_zones_are_converted() {
        let ics = cal("BEGIN:VEVENT\r\nUID:u\r\nDTSTART:20300610T120000Z\r\nSUMMARY:Utc\r\nEND:VEVENT\r\n");
        let ev = events_between(&ics, d(2030, 6, 9, 0, 0), d(2030, 6, 12, 0, 0));
        assert_eq!(ev[0].start, utc_at(2030, 6, 10, 12));

        // Rome in summer is UTC+2, in winter UTC+1.
        let rome = cal("BEGIN:VEVENT\r\nUID:r\r\nDTSTART;TZID=Europe/Rome:20300710T100000\r\nSUMMARY:Estate\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:w\r\nDTSTART;TZID=Europe/Rome:20301210T100000\r\nSUMMARY:Inverno\r\nEND:VEVENT\r\n");
        let ev = events_between(&rome, d(2030, 7, 1, 0, 0), d(2031, 1, 1, 0, 0));
        assert_eq!(ev[0].start, utc_at(2030, 7, 10, 8));
        assert_eq!(ev[1].start, utc_at(2030, 12, 10, 9));

        // New York in summer is UTC-4.
        let ny = cal("BEGIN:VEVENT\r\nUID:n\r\nDTSTART;TZID=America/New_York:20300710T100000\r\nSUMMARY:Ny\r\nEND:VEVENT\r\n");
        assert_eq!(events_between(&ny, d(2030, 7, 1, 0, 0), d(2030, 8, 1, 0, 0))[0].start, utc_at(2030, 7, 10, 14));

        // An unknown zone is read as the computer's time.
        let odd = cal("BEGIN:VEVENT\r\nUID:o\r\nDTSTART;TZID=Mars/Olympus:20300710T100000\r\nSUMMARY:Marte\r\nEND:VEVENT\r\n");
        assert_eq!(events_between(&odd, d(2030, 7, 1, 0, 0), d(2030, 8, 1, 0, 0))[0].start, d(2030, 7, 10, 10, 0));
    }

    fn utc_at(y: i32, m: u32, day: u32, h: u32) -> DateTime<Local> {
        Local.from_utc_datetime(&NaiveDate::from_ymd_opt(y, m, day).unwrap().and_hms_opt(h, 0, 0).unwrap())
    }

    #[test]
    fn dst_changes_follow_the_rules() {
        let eu = &CET;
        // 2030: last Sunday of March is the 31st; of October the 27th.
        let n = |m, day, h| NaiveDate::from_ymd_opt(2030, m, day).unwrap().and_hms_opt(h, 0, 0).unwrap();
        assert_eq!(eu.to_utc(n(3, 30, 12)), n(3, 30, 11));
        assert_eq!(eu.to_utc(n(4, 1, 12)), n(4, 1, 10));
        assert_eq!(eu.to_utc(n(10, 26, 12)), n(10, 26, 10));
        assert_eq!(eu.to_utc(n(10, 28, 12)), n(10, 28, 11));
        // US 2030: second Sunday of March is the 10th, first of November the 3rd.
        assert_eq!(US_E.to_utc(n(3, 9, 12)), n(3, 9, 17));
        assert_eq!(US_E.to_utc(n(3, 11, 12)), n(3, 11, 16));
        assert_eq!(US_E.to_utc(n(11, 4, 12)), n(11, 4, 17));
    }

    #[test]
    fn daily_weekly_monthly_and_yearly_rules() {
        // 2030-06-10 is a Monday.
        let daily =
            cal("BEGIN:VEVENT\r\nUID:d\r\nDTSTART:20300610T090000\r\nRRULE:FREQ=DAILY;INTERVAL=2;COUNT=3\r\nSUMMARY:D\r\nEND:VEVENT\r\n");
        let days: Vec<u32> = events_between(&daily, d(2030, 6, 1, 0, 0), d(2030, 7, 1, 0, 0)).iter().map(|e| e.start.day()).collect();
        assert_eq!(days, [10, 12, 14], "COUNT stops the series");

        let weekly = cal("BEGIN:VEVENT\r\nUID:w\r\nDTSTART:20300610T090000\r\nRRULE:FREQ=WEEKLY;BYDAY=MO,WE,FR;UNTIL=20300621T235959\r\nSUMMARY:W\r\nEND:VEVENT\r\n");
        let days: Vec<u32> = events_between(&weekly, d(2030, 6, 1, 0, 0), d(2030, 7, 1, 0, 0)).iter().map(|e| e.start.day()).collect();
        assert_eq!(days, [10, 12, 14, 17, 19, 21], "UNTIL is inclusive");

        let biweekly =
            cal("BEGIN:VEVENT\r\nUID:b\r\nDTSTART:20300610T090000\r\nRRULE:FREQ=WEEKLY;INTERVAL=2\r\nSUMMARY:B\r\nEND:VEVENT\r\n");
        let days: Vec<u32> = events_between(&biweekly, d(2030, 6, 1, 0, 0), d(2030, 7, 10, 0, 0)).iter().map(|e| e.start.day()).collect();
        assert_eq!(days, [10, 24, 8]);

        let monthly = cal("BEGIN:VEVENT\r\nUID:m\r\nDTSTART:20300131T090000\r\nRRULE:FREQ=MONTHLY;COUNT=4\r\nSUMMARY:M\r\nEND:VEVENT\r\n");
        let months: Vec<(u32, u32)> =
            events_between(&monthly, d(2030, 1, 1, 0, 0), d(2031, 1, 1, 0, 0)).iter().map(|e| (e.start.month(), e.start.day())).collect();
        assert_eq!(months, [(1, 31), (3, 31), (5, 31), (7, 31)], "months without a 31st are skipped");

        let yearly = cal("BEGIN:VEVENT\r\nUID:y\r\nDTSTART;VALUE=DATE:20200315\r\nRRULE:FREQ=YEARLY\r\nSUMMARY:Y\r\nEND:VEVENT\r\n");
        let ev = events_between(&yearly, d(2031, 3, 1, 0, 0), d(2031, 4, 1, 0, 0));
        assert_eq!((ev.len(), ev[0].start.year(), ev[0].all_day), (1, 2031, true));
    }

    #[test]
    fn exdate_and_overrides_and_cancelled_events() {
        let ics = cal(concat!(
            "BEGIN:VEVENT\r\nUID:s\r\nDTSTART:20300610T090000\r\nDTEND:20300610T100000\r\nRRULE:FREQ=DAILY;COUNT=4\r\nEXDATE:20300611T090000\r\nSUMMARY:Serie\r\nEND:VEVENT\r\n",
            "BEGIN:VEVENT\r\nUID:s\r\nRECURRENCE-ID:20300612T090000\r\nDTSTART:20300612T150000\r\nDTEND:20300612T160000\r\nSUMMARY:Serie spostata\r\nEND:VEVENT\r\n",
            "BEGIN:VEVENT\r\nUID:c\r\nDTSTART:20300610T110000\r\nSTATUS:CANCELLED\r\nSUMMARY:Annullato\r\nEND:VEVENT\r\n",
        ));
        let ev = events_between(&ics, d(2030, 6, 1, 0, 0), d(2030, 7, 1, 0, 0));
        let got: Vec<(u32, u32, &str)> = ev.iter().map(|e| (e.start.day(), e.start.hour(), e.summary.as_str())).collect();
        assert_eq!(got, [(10, 9, "Serie"), (12, 15, "Serie spostata"), (13, 9, "Serie")]);
    }

    use chrono::Timelike;

    #[test]
    fn an_alarm_inside_an_event_and_a_duration_are_understood() {
        let ics = cal("BEGIN:VEVENT\r\nUID:a\r\nDTSTART:20300610T090000\r\nDURATION:PT1H30M\r\nSUMMARY:Con allarme\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nSUMMARY:Non io\r\nTRIGGER:-PT15M\r\nEND:VALARM\r\nEND:VEVENT\r\n");
        let ev = events_between(&ics, d(2030, 6, 10, 0, 0), d(2030, 6, 11, 0, 0));
        assert_eq!(ev.len(), 1);
        assert_eq!((ev[0].summary.as_str(), ev[0].end), ("Con allarme", d(2030, 6, 10, 10, 30)));
    }

    #[test]
    fn rules_that_are_not_understood_show_only_the_first_occurrence() {
        let ics = cal("BEGIN:VEVENT\r\nUID:x\r\nDTSTART:20300610T090000\r\nRRULE:FREQ=MONTHLY;BYDAY=2MO\r\nSUMMARY:X\r\nEND:VEVENT\r\n");
        assert_eq!(events_between(&ics, d(2030, 6, 1, 0, 0), d(2031, 6, 1, 0, 0)).len(), 1);
    }

    #[test]
    fn garbage_is_not_an_error() {
        assert!(events_between(
            "non è un calendario\r\nBEGIN:VEVENT\r\nDTSTART:boh\r\nEND:VEVENT",
            d(2030, 1, 1, 0, 0),
            d(2031, 1, 1, 0, 0)
        )
        .is_empty());
    }
}
