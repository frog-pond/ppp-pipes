//! Reads the VEVENTs of a calendar into typed events (spec §5.2).

use jiff::civil;
use jiff::tz::TimeZone;
use jiff::{Span, Timestamp, Zoned};

use super::lines::{
    Component, Property, StructureError, parse_components, split_text_list, unescape_text,
};

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum IcsError {
    #[error("{0}")]
    Structure(#[from] StructureError),
    #[error(
        "event {uid}: {property} has a floating time and the calendar has no time zone; pass --tz"
    )]
    NoZone { uid: String, property: String },
    #[error("event {uid}: {property} names unknown time zone \"{tz}\"")]
    UnknownZone {
        uid: String,
        property: String,
        tz: String,
    },
    #[error("event {uid}: cannot read {property} value \"{value}\"")]
    BadValue {
        uid: String,
        property: String,
        value: String,
    },
    #[error("unknown time zone \"{0}\" given with --tz")]
    UnknownDefaultZone(String),
}

/// One VEVENT, with the columns of `nushell-ics/ics.nu` plus `tz`,
/// `rdates`, `exdates` and `recurrence_id`.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub uid: Option<String>,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub location: Option<String>,
    pub url: Option<String>,
    pub status: Option<String>,
    pub start: Option<Zoned>,
    pub end: Option<Zoned>,
    /// True when DTSTART is a DATE (`VALUE=DATE` or eight digits).
    pub all_day: bool,
    /// The IANA zone of DTSTART (or of the calendar, for UTC starts);
    /// `None` when no zone exists anywhere.
    pub tz: Option<String>,
    pub categories: Vec<String>,
    pub organizer: Option<Organizer>,
    pub geo: Option<(f64, f64)>,
    pub rrule: Option<String>,
    pub rdates: Vec<Zoned>,
    pub exdates: Vec<Zoned>,
    pub recurrence_id: Option<Zoned>,
    pub created: Option<Timestamp>,
    pub last_modified: Option<Timestamp>,
    /// Every property of the VEVENT, as read.
    pub props: Vec<Property>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Organizer {
    /// The `CN` parameter.
    pub name: Option<String>,
    /// The value with any `mailto:` removed.
    pub email: Option<String>,
    /// The `CUTYPE` parameter, upper-cased (e.g. `INDIVIDUAL`, `GROUP`).
    pub cutype: Option<String>,
}

/// Reads every VEVENT in `text`. `default_tz` (from `--tz`) is used only
/// when the calendar names no zone itself.
pub fn parse_events(text: &str, default_tz: Option<&str>) -> Result<Vec<Event>, IcsError> {
    if let Some(tz) = default_tz {
        TimeZone::get(tz).map_err(|_| IcsError::UnknownDefaultZone(tz.to_string()))?;
    }
    let mut events = Vec::new();
    for calendar in parse_components(text)? {
        let zone = calendar_zone(&calendar).or(default_tz.map(str::to_string));
        for component in calendar.components.iter().filter(|c| c.name == "VEVENT") {
            events.push(read_event(component, zone.as_deref())?);
        }
    }
    Ok(events)
}

/// The calendar's own zone: `X-WR-TIMEZONE`, else `TIMEZONE-ID`, else the
/// TZID of its only VTIMEZONE.
fn calendar_zone(calendar: &Component) -> Option<String> {
    for name in ["X-WR-TIMEZONE", "TIMEZONE-ID"] {
        if let Some(prop) = calendar.property(name) {
            return Some(prop.value.trim().to_string());
        }
    }
    let mut zones = calendar.components.iter().filter(|c| c.name == "VTIMEZONE");
    match (zones.next(), zones.next()) {
        (Some(only), None) => only.property("TZID").map(|p| p.value.trim().to_string()),
        _ => None,
    }
}

struct Reader<'a> {
    uid: String,
    calendar_zone: Option<&'a str>,
}

fn read_event(event: &Component, calendar_zone: Option<&str>) -> Result<Event, IcsError> {
    let uid = event.property("UID").map(|p| p.value.trim().to_string());
    let reader = Reader {
        uid: uid.clone().unwrap_or_else(|| "(no UID)".into()),
        calendar_zone,
    };
    let text = |name: &str| event.property(name).map(|p| unescape_text(&p.value));
    let raw = |name: &str| event.property(name).map(|p| p.value.trim().to_string());

    let start = event
        .property("DTSTART")
        .map(|p| reader.time(p))
        .transpose()?;
    let all_day = event.property("DTSTART").is_some_and(is_date);
    let end = match event.property("DTEND") {
        Some(p) => Some(reader.time(p)?),
        None => match (event.property("DURATION"), &start) {
            (Some(p), Some(start)) => Some(reader.add_duration(start, p)?),
            _ => None,
        },
    };
    let tz = match event.property("DTSTART") {
        Some(p) if is_date(p) && p.param("TZID").is_none() => calendar_zone.map(str::to_string),
        Some(p) => p
            .param("TZID")
            .map(str::to_string)
            .or(calendar_zone.map(str::to_string)),
        None => None,
    };

    Ok(Event {
        summary: text("SUMMARY"),
        description: text("DESCRIPTION"),
        location: text("LOCATION"),
        // URL is a URI, which RFC 5545 does not escape, but some feeds
        // (Carleton's) escape it like TEXT anyway; a URI never contains `\`.
        url: text("URL").map(|url| url.trim().to_string()),
        status: raw("STATUS"),
        start,
        end,
        all_day,
        tz,
        categories: event
            .all("CATEGORIES")
            .flat_map(|p| split_text_list(&p.value))
            .collect(),
        organizer: event.property("ORGANIZER").map(|p| Organizer {
            name: p.param("CN").map(str::to_string),
            email: Some(strip_mailto(&p.value)).filter(|e| !e.is_empty()),
            cutype: p.param("CUTYPE").map(str::to_ascii_uppercase),
        }),
        geo: event.property("GEO").and_then(|p| read_geo(&p.value)),
        rrule: raw("RRULE"),
        rdates: reader.times(event.all("RDATE"))?,
        exdates: reader.times(event.all("EXDATE"))?,
        recurrence_id: event
            .property("RECURRENCE-ID")
            .map(|p| reader.time(p))
            .transpose()?,
        // CREATED and LAST-MODIFIED are optional and often malformed, so a
        // bad value is skipped rather than failing the event.
        created: event
            .property("CREATED")
            .and_then(|p| reader.time(p).ok())
            .map(|z| z.timestamp()),
        last_modified: event
            .property("LAST-MODIFIED")
            .and_then(|p| reader.time(p).ok())
            .map(|z| z.timestamp()),
        uid,
        props: event.properties.clone(),
    })
}

impl Reader<'_> {
    /// Reads a DATE or DATE-TIME property value into a zoned time.
    fn time(&self, prop: &Property) -> Result<Zoned, IcsError> {
        self.time_value(prop, prop.value.trim())
    }

    /// Reads every value of every given RDATE/EXDATE-style property.
    fn times<'p>(&self, props: impl Iterator<Item = &'p Property>) -> Result<Vec<Zoned>, IcsError> {
        let mut out = Vec::new();
        for prop in props {
            for value in prop
                .value
                .split(',')
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                out.push(self.time_value(prop, value)?);
            }
        }
        Ok(out)
    }

    fn time_value(&self, prop: &Property, value: &str) -> Result<Zoned, IcsError> {
        let bad = || IcsError::BadValue {
            uid: self.uid.clone(),
            property: prop.name.clone(),
            value: value.to_string(),
        };
        if prop.param("VALUE") == Some("DATE") || is_date_text(value) {
            let date = civil::Date::strptime("%Y%m%d", value).map_err(|_| bad())?;
            // A date belongs to no zone (RFC 5545). Use the event's zone when
            // there is one; otherwise midnight UTC, which writes back as the
            // same date (spec §5.3).
            let zone = match self.zone_name(prop) {
                Some(name) => self.zone(prop, &name)?,
                None => TimeZone::UTC,
            };
            return date.to_zoned(zone).map_err(|_| bad());
        }
        let (wall, utc) = match value.strip_suffix('Z') {
            Some(wall) => (wall, true),
            None => (value, false),
        };
        let wall = civil::DateTime::strptime("%Y%m%dT%H%M%S", wall).map_err(|_| bad())?;
        let zone = if utc {
            TimeZone::UTC
        } else {
            let name = self.zone_name(prop).ok_or_else(|| IcsError::NoZone {
                uid: self.uid.clone(),
                property: prop.name.clone(),
            })?;
            self.zone(prop, &name)?
        };
        wall.to_zoned(zone).map_err(|_| bad())
    }

    fn zone_name(&self, prop: &Property) -> Option<String> {
        prop.param("TZID")
            .map(str::to_string)
            .or(self.calendar_zone.map(str::to_string))
    }

    fn zone(&self, prop: &Property, name: &str) -> Result<TimeZone, IcsError> {
        TimeZone::get(name).map_err(|_| IcsError::UnknownZone {
            uid: self.uid.clone(),
            property: prop.name.clone(),
            tz: name.to_string(),
        })
    }

    fn add_duration(&self, start: &Zoned, prop: &Property) -> Result<Zoned, IcsError> {
        let bad = || IcsError::BadValue {
            uid: self.uid.clone(),
            property: prop.name.clone(),
            value: prop.value.clone(),
        };
        let span: Span = prop.value.trim().parse().map_err(|_| bad())?;
        start.checked_add(span).map_err(|_| bad())
    }
}

fn is_date(prop: &Property) -> bool {
    prop.param("VALUE") == Some("DATE") || is_date_text(prop.value.trim())
}

fn is_date_text(value: &str) -> bool {
    value.len() == 8 && value.bytes().all(|b| b.is_ascii_digit())
}

fn strip_mailto(value: &str) -> String {
    let value = value.trim();
    match value.get(..7) {
        Some(prefix) if prefix.eq_ignore_ascii_case("mailto:") => value[7..].to_string(),
        _ => value.to_string(),
    }
}

fn read_geo(value: &str) -> Option<(f64, f64)> {
    let (lat, lon) = value.split_once(';')?;
    Some((lat.trim().parse().ok()?, lon.trim().parse().ok()?))
}
