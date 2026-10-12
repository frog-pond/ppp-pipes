# `calendar.schema.json`

`$id`: `https://pipes.frogpond.tech/stdlib/calendar/1` · version 1.0.0

This schema defines one record type, `event` (`…/calendar/1#event`). Its fields follow iCalendar's `VEVENT`. The types it names, such as `when`, `url` and `agent`, come from [`common.schema.json`](common.schema.md).

The smallest valid event:

```json
{ "type": "event", "id": "dentist-2026-10-20", "summary": "Dentist", "start": "2026-10-20T08:15:00Z" }
```

## `event`

An event has the [`record`](common.schema.md#record) fields (`type`, `id`, `source`, `partial`, `href`, `extra`), with `type` always `"event"`. An event that is not `partial` also needs `summary` and `start`.

The event is closed: any field not listed here fails validation. Put source fields the schema does not model in `extra`.

### What and when

| Field | Type | Meaning |
|---|---|---|
| `summary` | `text` | The title. |
| `start` | `when` | When it starts. |
| `end` | `when` | When it ends. The end is exclusive: an all-day event on 14 October has `start: "2026-10-14"` and `end: "2026-10-15"`. |
| `duration` | `duration` | How long it lasts, instead of `end`. An event may have `end` or `duration`, not both. With neither, the event is a moment. |
| `all_day` | boolean | The event fills whole days. Its `start`, `end`, `recurrence_id`, `exdates` and `rdates` must then be dates. |
| `tz` | `tz` | The zone the event happens in. Required when any time on the event is a local date-time. |
| `status` | `"tentative"`, `"confirmed"` or `"cancelled"` | As in iCalendar's `STATUS`. |

#### How times are written

`start`, `end`, `recurrence_id`, the items of `exdates`, and the `start` and `end` of each item of `rdates` are `when` fields, so each can be a date, an instant or a local date-time. Within one event they follow one rule, which `ppp` applies when it writes the record:

- `all_day: true` → dates: `"2026-10-14"`;
- else, with `tz` → local date-times in that zone: `"2026-10-14T19:30:00"`;
- else → instants in UTC: `"2026-10-15T00:30:00Z"`.

Records written by hand, or by tools other than `ppp`, may mix forms. The schema allows that, as long as any local date-time comes with a `tz`.

### Where

| Field | Type | Meaning |
|---|---|---|
| `location` | object | Where it happens. Needs `name`, `place`, or both. |
| `location.name` | `text` | The location as the source wrote it, such as `"Tomson 280"`. |
| `location.place` | `place` | The place in full, when the source gives an address or other details. |
| `location.geo` | `geo` | A point, when the source gives one but no full place. |
| `attendance` | `"in-person"`, `"online"` or `"hybrid"` | How people attend. |
| `online` | object | Where to attend online: `url` (required) and `platform`, such as `"zoom"`. |

### Description and links

| Field | Type | Meaning |
|---|---|---|
| `excerpt` | `text` | A short plain-text summary, for lists. |
| `description` | `text` | The full description as plain text. |
| `content` | `content` | The full description with its markup (HTML or Markdown), when the source has some. |
| `url` | `url` | The event's own page. |
| `links` | `link[]` | Other pages, such as tickets or registration. |
| `images` | `image[]` | Pictures of the event. |
| `cost` | `money` | What it costs to attend. |
| `categories` | `categories` | |
| `keywords` | `keywords` | |

### People

| Field | Type | Meaning |
|---|---|---|
| `organizers` | `agent[]` | Who runs it: each item is `{"person": …}` or `{"org": …}`, with nothing else. |
| `attendees` | `agent[]` plus `role` and `rsvp` | Who is invited. `role` is `"chair"`, `"required"` or `"optional"`; `rsvp` is `"needs-action"`, `"accepted"`, `"declined"` or `"tentative"`. |

```json
"attendees": [
  { "person": { "name": { "display": "Sam Okafor" } }, "role": "required", "rsvp": "needs-action" }
]
```

### Repeating events

An event that repeats can arrive in two shapes.

**As a rule.** The event carries `rrule`, and maybe `rdates` and `exdates`. Its `start` is the first occurrence.

| Field | Type | Meaning |
|---|---|---|
| `rrule` | `rrule` | How the event repeats, as in RFC 5545. |
| `rdates` | list of `{start, end}` | Extra occurrences the rule does not produce. `start` (a `when`) is required. `end` (a `when`) is optional: without it, the occurrence lasts as long as the event. |
| `exdates` | `when[]` | Occurrences the rule produces that do not happen. |

**As occurrences.** Each occurrence is its own event. This is what `expand rrule` produces, and what feeds like Google Calendar's give.

| Field | Type | Meaning |
|---|---|---|
| `series` | `text` | The `id` of the event the occurrence belongs to. |
| `recurrence_id` | `when` | Which occurrence this is: its start as the rule placed it. It differs from `start` when one occurrence has moved. |

`into event` gives each occurrence the id `<series>_<recurrence_id in UTC>`, for example `abc_20261013T170000Z`, which is the form Google uses. So the same occurrence gets the same id whether `ppp` expanded the rule or the source did.

### Everything else

| Field | Type | Meaning |
|---|---|---|
| `reminders` | object[] | Alerts to offer: `before` (a `duration`, required) and `method`, `"display"` or `"email"`. |
| `created` | `instant` | When the source created the event. |
| `modified` | `instant` | When the source last changed it. |
| `featured` | boolean | The source highlights this event. |
| `hidden` | boolean | The source keeps this event out of its listings. |

## Rules that span fields

Most fields check only themselves. These rules look at more than one:

1. Unless `partial` is `true`, `summary` and `start` are required.
2. `end` and `duration` cannot both appear.
3. If `all_day` is `true`, `start`, `end`, `recurrence_id`, every item of `exdates`, and the `start` and `end` of every item of `rdates` must be dates.
4. If `start`, `end`, `recurrence_id`, any item of `exdates`, or the `start` or `end` of any item of `rdates` is a local date-time, `tz` is required.
5. `location` needs `name` or `place`.
