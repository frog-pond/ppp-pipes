# `common.schema.json`

`$id`: `https://pipes.frogpond.tech/stdlib/common/1`

This schema defines no record type of its own. It holds the building blocks that record types share, such as times, links, people and places. A record type refers to one with `$ref`, for example `https://pipes.frogpond.tech/stdlib/common/1#/$defs/url`. The only record type today is [`event`](calendar.schema.md).

Every object below is closed: a field the schema does not name fails validation. The one exception is `agent`, explained under [People and organizations](#people-and-organizations).

## Records

### `record`

The fields every record has. A record type merges it in with `allOf`.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `type` | string | yes | The record type, such as `event`. Lowercase letters, digits and dashes, starting with a letter. |
| `id` | `text` | yes | Unique within the source. Stable across runs, so a client can tell a changed record from a new one. |
| `source` | `url` | | Where the record came from. |
| `partial` | boolean | | The record leaves out fields the full record would have, as search results often do. A partial record need not carry its type's required fields (beyond `type` and `id`). |
| `href` | `url` | | Where to get the full record, usually set with `partial`. |
| `extra` | object | | Source fields the schema does not model, kept so no data is lost. Anything goes inside it. |

## Text

### `text`

A string that is not empty. Use it for names and titles, where an empty string would be a mistake.

### `content`

A body of text and the format it is written in.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `type` | `"text"`, `"html"` or `"markdown"` | yes | How to read `value`. |
| `value` | string | yes | The body. May be empty. |
| `lang` | `text` | | Its language, such as `en`. |

## Time

A pipe works with nushell datetimes, which always carry an offset. When a run writes records out, it formats each time field by the definition the schema gives it. So these definitions say both what a valid value looks like and how `ppp` writes one.

### `date`

A calendar date, `2026-10-14`. `ppp` takes the date in the record's `tz`. A record with no `tz` gets the date in the datetime's own offset, which keeps the date the source gave.

### `instant`

A moment in time, written in RFC 3339 with an offset: `2026-10-14T00:30:00Z`. `ppp` always writes instants in UTC. Fractions of a second are allowed when reading but `ppp` does not write them.

### `local-date-time`

A wall-clock time with no offset, `2026-10-14T19:30:00`. It means nothing on its own: a record that has one must also have `tz`, which says where the clock hangs. `ppp` writes it as the wall time in the record's `tz`.

### `when`

Any of `date`, `instant` or `local-date-time`. A field typed `when` lets each record pick the form that fits it. `ppp` picks by this rule:

1. `date`, if the record has `all_day: true`;
2. otherwise `local-date-time`, if the record has `tz`;
3. otherwise `instant`.

Keeping the wall time and the zone, rather than only an instant, matters for events that repeat: an 18:00 class stays at 18:00 when daylight saving time starts.

### `tz`

An IANA time zone name, such as `America/Chicago`. `ppp` checks it against its time zone database (the `iana-tz` format).

### `duration`

An ISO 8601 duration, such as `PT1H30M` or `P1D`. `ppp` drops fractions of a second and writes hours, minutes and seconds: a nushell `1hr 30min` becomes `PT1H30M`, and `1day` becomes `PT24H`.

### `rrule`

An RFC 5545 recurrence rule, such as `FREQ=WEEKLY;BYDAY=TU,TH;UNTIL=20261215T000000Z`. The schema checks only that it has a valid `FREQ`; `expand rrule` reads the rest.

### `created` and `modified`

When the source says it created or last changed the record. Both are `instant`s.

## Links and contact details

### `url`

An absolute `http` or `https` URL. It must be a valid URI, so characters outside ASCII must be percent-encoded. `into event` does that for you.

### `link`

A URL with some words about it.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `url` | `url` | yes | |
| `rel` | string | | How it relates to the record, like HTML's `rel`: `tickets`, `registration`. Lowercase. |
| `title` | `text` | | Text to show for the link. |
| `type` | `text` | | The media type of what it points to, such as `text/html`. |

### `image`

| Field | Type | Required | Meaning |
|---|---|---|---|
| `url` | `url` | yes | |
| `alt` | string | | Text that stands in for the image. May be empty for a purely decorative image. |
| `width`, `height` | positive integer | | Size in pixels. |
| `caption` | `text` | | |
| `credit` | `text` | | Who made the image. |

### `email`

| Field | Type | Required | Meaning |
|---|---|---|---|
| `address` | string | yes | A valid email address. |
| `kind` | `"work"`, `"home"` or `"other"` | | |

### `phone`

| Field | Type | Required | Meaning |
|---|---|---|---|
| `number` | string | yes | At least three of: digits, spaces, and `+ ( ) . / x -`. The schema does not check that the number exists. |
| `kind` | `"work"`, `"home"`, `"mobile"`, `"fax"` or `"other"` | | |

## Places

### `geo`

A point on the earth: `lat` from −90 to 90 and `lon` from −180 to 180, both required.

### `address`

A postal address. It needs at least one field.

| Field | Type | Meaning |
|---|---|---|
| `lines` | `text[]`, at least one | The street lines. |
| `locality` | `text` | The city or town. |
| `region` | `text` | The state or province. |
| `postcode` | `text` | |
| `country` | string | Two capital letters, as in ISO 3166: `US`. |

### `place`

A named place.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `name` | `text` | yes | |
| `id` | `text` | | The source's id for the place. |
| `address` | `address` | | |
| `geo` | `geo` | | |
| `url` | `url` | | |
| `phones` | `phone[]` | | |
| `room` | `text` | | A room inside the place. |

## People and organizations

### `person`

| Field | Type | Required | Meaning |
|---|---|---|---|
| `name` | object | yes | `display` (required), plus `given` and `family` when the source splits them. |
| `id` | `text` | | |
| `emails` | `email[]` | | |
| `phones` | `phone[]` | | |
| `url` | `url` | | |

### `org`

| Field | Type | Required | Meaning |
|---|---|---|---|
| `name` | `text` | yes | |
| `short_name` | `text` | | Such as `SGA`. |
| `id` | `text` | | |
| `emails` | `email[]` | | |
| `phones` | `phone[]` | | |
| `url` | `url` | | |

### `agent`

Someone who does something, such as an organizer: either `{"person": …}` or `{"org": …}`, never both.

```json
{ "org": { "name": "Student Activities", "url": "https://example.edu/sao" } }
```

`agent` is the one open object here, so that a field using it can add its own keys. An event's `attendees` add `role` and `rsvp` this way. Each field that uses `agent` closes it again, so a typo still fails.

## Money

### `money`

A price. Either free:

```json
{ "free": true, "note": "Bring a canned good" }
```

or an amount:

| Field | Type | Required | Meaning |
|---|---|---|---|
| `min` | number ≥ 0 | yes | The lowest price, or the only one. |
| `max` | number ≥ 0 | | The highest price, for a range. |
| `currency` | string | yes | Three capital letters, as in ISO 4217: `USD`. |
| `per` | `"hour"`, `"week"`, `"semester"` or `"item"` | | What one price buys. Leave it out for a one-time price. |
| `note` | `text` | | |

## Tags

### `categories` and `keywords`

Lists of `text`, with at least one item and no repeats. Leave the field out rather than give an empty list.
