//! `expand rrule`: one row per occurrence of each recurring event (spec §5.4).

use std::collections::HashSet;

use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp, Zoned};
use nu_engine::command_prelude::*;
use nu_protocol::shell_error::generic::GenericError;

use crate::recur::{Series, occurrences};
use crate::time;

#[derive(Clone)]
pub struct ExpandRrule;

impl Command for ExpandRrule {
    fn name(&self) -> &str {
        "expand rrule"
    }

    fn description(&self) -> &str {
        "Replace each recurring event with one row per occurrence that overlaps a window."
    }

    fn extra_description(&self) -> &str {
        "Works on `from ics` rows. A row with `rrule` or `rdates` becomes one row per occurrence \
         overlapping [--from, --until), each with `recurrence_id` set to the occurrence's original \
         start and no `rrule`, `rdates` or `exdates`. An occurrence that has an override row \
         (same `uid`, matching `recurrence_id`) is left out, since the override row stands in \
         for it. Every other row passes through unchanged; filter them with `during`."
    }

    fn signature(&self) -> Signature {
        Signature::build("expand rrule")
            .input_output_types(vec![(Type::table(), Type::table())])
            .required_named("from", SyntaxShape::DateTime, "Start of the window", None)
            .required_named(
                "until",
                SyntaxShape::DateTime,
                "End of the window (exclusive)",
                None,
            )
            .category(Category::Filters)
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let head = call.head;
        let from: Value = call
            .get_flag(engine_state, stack, "from")?
            .expect("required flag");
        let until: Value = call
            .get_flag(engine_state, stack, "until")?
            .expect("required flag");
        let out_of_range = |value: &Value| {
            ShellError::Generic(GenericError::new(
                "date out of range",
                "outside the years -9999 to 9999",
                value.span(),
            ))
        };
        let from_ts = time::timestamp(&from.as_date()?).map_err(|_| out_of_range(&from))?;
        let until_ts = time::timestamp(&until.as_date()?).map_err(|_| out_of_range(&until))?;
        let rows = input.into_value(head)?.into_list()?;
        let expanded = expand(rows, from_ts, until_ts, head).map_err(|err| {
            ShellError::Generic(GenericError::new(
                format!("cannot expand event {}", err.uid),
                err.message,
                head,
            ))
        })?;
        Ok(Value::list(expanded, head).into_pipeline_data())
    }
}

/// Why one event could not be expanded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpandError {
    pub uid: String,
    pub message: String,
}

/// Expands every recurring row in `rows` over [from, until).
pub fn expand(
    rows: Vec<Value>,
    from: Timestamp,
    until: Timestamp,
    span: Span,
) -> Result<Vec<Value>, ExpandError> {
    let overrides: HashSet<(String, Timestamp)> = rows
        .iter()
        .filter_map(|row| {
            Some((
                text(row, "uid")?,
                timestamp(&row.get_data_by_key("recurrence_id")?)?,
            ))
        })
        .collect();
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        if !is_recurring(&row) {
            out.push(row);
            continue;
        }
        let uid = text(&row, "uid").unwrap_or_else(|| "(no uid)".into());
        let fail = |message: String| ExpandError {
            uid: uid.clone(),
            message,
        };
        let zone = match text(&row, "tz") {
            Some(name) => {
                TimeZone::get(&name).map_err(|_| fail(format!("unknown time zone \"{name}\"")))?
            }
            None => TimeZone::UTC,
        };
        let start = row
            .get_data_by_key("start")
            .and_then(|v| timestamp(&v))
            .ok_or_else(|| fail("a recurring event needs a start".into()))?
            .to_zoned(zone.clone());
        let end = row
            .get_data_by_key("end")
            .and_then(|v| timestamp(&v))
            .map(|end| end.to_zoned(zone.clone()));
        let length = end.as_ref().map_or(SignedDuration::ZERO, |end| {
            end.timestamp().duration_since(start.timestamp())
        });
        // An all-day event lasts whole days, which are not all 24 hours long.
        let all_day = matches!(
            row.get_data_by_key("all_day"),
            Some(Value::Bool { val: true, .. })
        );
        let occurrence_length = end.as_ref().map(|end| match all_day {
            true => Length::Days(jiff::Span::new().days((end.date() - start.date()).get_days())),
            false => Length::Exact(length),
        });
        let in_zone = |key: &str| -> Vec<Zoned> {
            match row.get_data_by_key(key) {
                Some(Value::List { vals, .. }) => vals
                    .iter()
                    .filter_map(timestamp)
                    .map(|ts| ts.to_zoned(zone.clone()))
                    .collect(),
                _ => Vec::new(),
            }
        };
        let (rdates, exdates) = (in_zone("rdates"), in_zone("exdates"));
        let rrule = text(&row, "rrule");
        let series = Series {
            start: &start,
            rrule: rrule.as_deref(),
            rdates: &rdates,
            exdates: &exdates,
        };
        for occurrence in
            occurrences(&series, length, from, until).map_err(|e| fail(e.to_string()))?
        {
            if !overrides.contains(&(uid.clone(), occurrence.timestamp())) {
                out.push(occurrence_row(&row, &occurrence, occurrence_length, span));
            }
        }
    }
    Ok(out)
}

fn text(row: &Value, key: &str) -> Option<String> {
    match row.get_data_by_key(key)? {
        Value::String { val, .. } => Some(val),
        _ => None,
    }
}

fn timestamp(value: &Value) -> Option<Timestamp> {
    time::timestamp(&value.as_date().ok()?).ok()
}

fn is_recurring(row: &Value) -> bool {
    let has_rdates = matches!(
        row.get_data_by_key("rdates"),
        Some(Value::List { vals, .. }) if !vals.is_empty()
    );
    text(row, "rrule").is_some() || has_rdates
}

/// How far an occurrence's end is from its start.
#[derive(Clone, Copy)]
enum Length {
    Exact(SignedDuration),
    /// Calendar days, for all-day events.
    Days(jiff::Span),
}

/// The master row with its times moved to one occurrence.
fn occurrence_row(master: &Value, start: &Zoned, length: Option<Length>, span: Span) -> Value {
    let mut record = master.as_record().cloned().unwrap_or_default();
    let at = Value::date(time::to_nu(start), span);
    record.insert("start", at.clone());
    if let Some(length) = length {
        let end = match length {
            Length::Exact(duration) => start.checked_add(duration),
            Length::Days(days) => start.checked_add(days),
        }
        .expect("an occurrence's end is in range");
        record.insert("end", Value::date(time::to_nu(&end), span));
    }
    record.insert("recurrence_id", at);
    record.insert("rrule", Value::nothing(span));
    record.insert("rdates", Value::list(Vec::new(), span));
    record.insert("exdates", Value::list(Vec::new(), span));
    Value::record(record, span)
}
