//! `from ics`: iCalendar text to a table of events (spec §5.2).

use nu_engine::command_prelude::*;
use nu_protocol::shell_error::generic::GenericError;

use crate::ics::events::{Event, parse_events};
use crate::ics::lines::Property;
use crate::time;

#[derive(Clone)]
pub struct FromIcs;

impl Command for FromIcs {
    fn name(&self) -> &str {
        "from ics"
    }

    fn description(&self) -> &str {
        "Parse iCalendar text into a table of events, one row per VEVENT."
    }

    fn extra_description(&self) -> &str {
        "Floating times use the calendar's X-WR-TIMEZONE, TIMEZONE-ID or only VTIMEZONE, then --tz. \
         All-day dates with no zone anywhere become midnight UTC."
    }

    fn signature(&self) -> Signature {
        Signature::build("from ics")
            .input_output_types(vec![
                (Type::String, Type::table()),
                (Type::Binary, Type::table()),
            ])
            .named(
                "tz",
                SyntaxShape::String,
                "Zone for floating times when the calendar names none",
                None,
            )
            .category(Category::Formats)
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let head = call.head;
        let tz: Option<String> = call.get_flag(engine_state, stack, "tz")?;
        // Feeds served as application/octet-stream arrive as binary.
        let value = input.into_value(head)?;
        let span = value.span();
        let text = match value {
            Value::String { val, .. } => val,
            Value::Binary { val, .. } => String::from_utf8(val.to_vec()).map_err(|_| {
                ShellError::Generic(GenericError::new(
                    "cannot read iCalendar text",
                    "the input is not UTF-8 text",
                    span,
                ))
            })?,
            other => {
                return Err(ShellError::OnlySupportsThisInputType {
                    exp_input_type: "string or binary".into(),
                    wrong_type: other.get_type().to_string(),
                    dst_span: head,
                    src_span: span,
                });
            }
        };
        let events = parse_events(&text, tz.as_deref()).map_err(|err| {
            ShellError::Generic(GenericError::new(
                "cannot read iCalendar text",
                err.to_string(),
                span,
            ))
        })?;
        let rows = events
            .iter()
            .map(|event| event_to_value(event, head))
            .collect();
        Ok(Value::list(rows, head).into_pipeline_data())
    }
}

fn event_to_value(event: &Event, span: Span) -> Value {
    let text = |value: &Option<String>| match value {
        Some(text) => Value::string(text.clone(), span),
        None => Value::nothing(span),
    };
    let date = |value: &Option<jiff::Zoned>| match value {
        Some(zoned) => Value::date(time::to_nu(zoned), span),
        None => Value::nothing(span),
    };
    let instant = |value: &Option<jiff::Timestamp>| match value {
        Some(ts) => Value::date(time::to_nu(&ts.to_zoned(jiff::tz::TimeZone::UTC)), span),
        None => Value::nothing(span),
    };
    let dates = |values: &[jiff::Zoned]| {
        Value::list(
            values
                .iter()
                .map(|z| Value::date(time::to_nu(z), span))
                .collect(),
            span,
        )
    };
    Value::record(
        record! {
            "summary" => text(&event.summary),
            "start" => date(&event.start),
            "end" => date(&event.end),
            "all_day" => Value::bool(event.all_day, span),
            "tz" => text(&event.tz),
            "location" => text(&event.location),
            "description" => text(&event.description),
            "url" => text(&event.url),
            "categories" => Value::list(event.categories.iter().map(|c| Value::string(c.clone(), span)).collect(), span),
            "organizer" => match &event.organizer {
                Some(org) => Value::record(
                    record! {
                        "name" => text(&org.name),
                        "email" => text(&org.email),
                        "cutype" => text(&org.cutype),
                    },
                    span,
                ),
                None => Value::nothing(span),
            },
            "geo" => match event.geo {
                Some((lat, lon)) => Value::record(
                    record! { "lat" => Value::float(lat, span), "lon" => Value::float(lon, span) },
                    span,
                ),
                None => Value::nothing(span),
            },
            "uid" => text(&event.uid),
            "status" => text(&event.status),
            "rrule" => text(&event.rrule),
            "rdates" => dates(&event.rdates),
            "exdates" => dates(&event.exdates),
            "recurrence_id" => date(&event.recurrence_id),
            "created" => instant(&event.created),
            "last_modified" => instant(&event.last_modified),
            "props" => Value::list(event.props.iter().map(|p| property_to_value(p, span)).collect(), span),
        },
        span,
    )
}

fn property_to_value(prop: &Property, span: Span) -> Value {
    let mut params = Record::new();
    for (key, value) in &prop.params {
        params.push(key.clone(), Value::string(value.clone(), span));
    }
    Value::record(
        record! {
            "name" => Value::string(prop.name.clone(), span),
            "params" => Value::record(params, span),
            "value" => Value::string(prop.value.clone(), span),
        },
        span,
    )
}
