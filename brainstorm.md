# Standard library schemas

The "standard library" of record types a pipe can emit, plus the conventions they share.

- **Pipes are nushell modules** (§6). Their transform is an ordinary nushell pipeline.
- **A pipe outputs a list of self-contained records**: a nushell table in memory, and a JSON array or JSONL on the wire. There's no document root or header, so `$a | append $b` just works.
- **Records are validated against JSON Schema 2020-12.** Each record's `type` names its schema definition.
- **JSON is the only output format.**

Nothing here is a schema yet. The JSON in §3 is illustrative, and `…` marks elided values.

---

## 0. Data model

### 0.1 Record conventions

- **Output is a list of records.** There's no wrapping object.
- **Every record has `type` and `id`.** `type` is the record type's short name and is unique across the stdlib (`event`, `entry`, `person`, `org`, `trip`, `game`, `word`, `station`, …). It matches the `$anchor` of the record's `$def` (§1.6). `id` is stable within the source.
- **Records are self-contained.** Everything a record needs travels inside it: its own `tz` for local times, its own `source` URL, and shared things like venues, people, teams and stops **embedded** as objects rather than referenced by id. Embedded objects keep their own `id` so consumers can dedupe them. This costs some duplication, and in return filtering never strands a reference, merging two pipes needs no id reconciliation, and streaming needs no ordering rules.
- **Field names are snake_case.** Repeatable fields are arrays with plural names (`categories`, `links`, `organizers`). Single fields are scalars or objects.
- **People, orgs and places are embedded objects** wherever they appear, either full or partial (§1.3).
- Enum values are lowercase kebab-case, and durations are ISO 8601 (`"PT1H"`). Dates follow §0.2.
- Records are closed (`unevaluatedProperties: false`), except for an optional `extra` object that holds source fields we don't model (§1.5).

Here are two records from a calendar pipe:

```json
[
  { "type": "event", "id": "20261014-orchestra@calendar.example.edu",
    "source": "https://www.stolaf.edu/apps/calendar/ical.cfm",
    "summary": "Fall Orchestra Concert",
    "start": "2026-10-14T19:30:00", "end": "2026-10-14T21:00:00", "tz": "America/Chicago",
    "location": { "place": { "id": "boe-chapel", "name": "Boe Memorial Chapel",
                             "address": { "lines": ["1520 St. Olaf Avenue"], "locality": "Northfield", "region": "MN", "country": "US" },
                             "geo": { "lat": 44.4614, "lon": -93.1822 } } },
    "categories": ["Music", "Concert"] },
  { "type": "event", "id": "weekly-chess@calendar.example.edu",
    "summary": "Chess Club",
    "start": "2026-09-08T18:00:00", "duration": "PT2H", "tz": "America/Chicago",
    "location": { "name": "Buntrock Commons, Room 143" },
    "rrule": "FREQ=WEEKLY;BYDAY=TU;UNTIL=20261215T235959Z",
    "exdates": ["2026-11-24T18:00:00"] }
]
```

### 0.2 Dates and times

JSON Schema's `format: "date-time"` follows RFC 3339, which **requires** an offset. Calendars need three kinds of time, so `common` defines three:

| `$def` | Example | Check |
|---|---|---|
| `date` | `"2026-10-17"` | `format: date` |
| `instant` | `"2026-10-22T16:00:00Z"`, `"…-05:00"` | `format: date-time` |
| `local-date-time` | `"2026-10-14T19:30:00"`, plus `"tz": "America/Chicago"` on the record | pattern, no offset |

`when` accepts any of the three. A record with a local time must carry `tz` itself, enforced with `if`/`then` in the schema. In 2020-12, `format` is only an annotation unless the validator turns on format assertion, so the validator turns it on **and** the schemas keep patterns as well. That way validation doesn't depend on validator settings.

---

## 1. Structure

### 1.1 Sharing definitions across schemas

This uses JSON Schema's own mechanism:

- **Each schema has an `$id` URI** with the major version in it, like `https://pipes.dev/stdlib/common/1`. A breaking change gets a new URI. (The host name is a placeholder.)
- **`$ref` works across schemas**: `{"$ref": "https://pipes.dev/stdlib/common/1#/$defs/person"}`.
- **Schemas are resolved from a registry, never fetched.** The validator is preloaded with every stdlib schema, keyed by `$id`, and an unknown URI is an error. Validation stays offline and deterministic.
- **Bundling** (2020-12 compound documents) is available for anyone who wants a single file, using standard tools.
- **Building one record type from another**, such as an athletics `game` from a calendar `event`: `allOf: [{"$ref": "…/calendar/1#event"}]` plus the extra properties, with `unevaluatedProperties: false` so the combined type stays closed.

Schemas live at `stdlib/<name>.schema.json`.

### 1.2 Pipe-level information

Whatever describes a pipe rather than a record lives on the pipe or with the host:

| Information | Where it lives |
|---|---|
| Record types emitted | the pipe's declared `output` (§6.1), plus each record's `type` |
| Title, description | the pipe's doc comment |
| Time zone | each record's `tz` |
| Source URL | each record's `source`, so merged outputs keep per-record provenance |
| Fetch time, run id, totals | recorded by the host for each run |
| License | the pipe's `pipe` const |

**Edge envelope.** A web client fetching one pipe may want a single object that includes the title and fetch time. The host can wrap *any* pipe's output in one generic envelope (`$defs/envelope` in `common`). The envelope is the same for every schema, and pipes never see it:

```json
{ "pipe": "campus-week", "title": "This week on campus", "fetched": "2026-10-12T00:00:00Z",
  "output": ["https://pipes.dev/stdlib/calendar/1#event"],
  "records": [ … ] }
```

Search metadata (the query, total count, truncation, next page) isn't record data either. A pipe pages through results itself using its own parameters. If a consumer needs to know there were more results, that's an envelope field.

### 1.3 Single records, collections and partial records

A directory search and a single contact are the same thing: a list of `person` and `org` records, whether that's one record or many. Mixed outputs work because every record has a `type`, and `where type == person` splits them.

Search results often carry less than the detail page does. Any record may have `"partial": true` and an optional `"href"` that points to the full record, so no separate "summary" types are needed. A partial record relaxes `required` (`if partial then … else …`).

### 1.4 Rich text

Plain-text fields are strings. Fields that can carry markup use a `content` object, `{"type": "html", "value": "<p>…</p>"}`, where `type` is one of `text | html | markdown` (Atom's approach). Consumers sanitize the HTML.

Dictionary definitions are the one place a pipe needs to *see inside* the markup, to follow cross-references. So a definition may also be an array of runs:

```json
"definition": [ { "text": "The edge of a " }, { "xref": "river", "sense": "river-n-1" }, { "text": " or lake." } ]
```

### 1.5 Unknown fields

Records are closed (`unevaluatedProperties: false`), so typos fail validation. Each record may have one open `extra` object for source fields we don't model, so no data is lost.

### 1.6 Versioning and record type names

The major version is part of the schema's `$id` (`…/calendar/1`). Minor and patch versions go in the schema's `"version"` annotation. Each record type's `$def` has an `$anchor` equal to its `type`, so `https://pipes.dev/stdlib/calendar/1#event` names the event shape. A pipe declares the record types it emits as a list of these URIs (§6.1), and validation picks the right `$def` from each record's `type`. Because record type names are unique across the stdlib, `type` alone is unambiguous.

### 1.7 A common "item" projection

Any operator worked on any source in Yahoo Pipes because everything was an RSS item. To keep that property, each domain ships a nushell `to-item` that maps its records onto a minimal `item` record (`$defs/item` in `common`):

```json
{ "type": "item", "id": "…", "title": "…", "link": "https://…", "summary": "…",
  "date": "2026-10-14T19:30:00", "tz": "America/Chicago",
  "categories": ["…"], "image": { "url": "https://…", "alt": "…" } }
```

`date` is the one date that matters for sorting. Generic operators like sort, filter, dedupe, union, truncate and merge-into-a-feed work on items. Domain-aware operators use the full record.

### 1.8 Streaming with JSONL

Output is a list of self-contained records, so streaming is simply one record per line. There's no header line and no ordering rule:

```jsonl
{"type":"event","id":"20261014-orchestra@…","summary":"Fall Orchestra Concert","start":"2026-10-14T19:30:00","tz":"America/Chicago","location":{"place":{"id":"boe-chapel","name":"Boe Memorial Chapel"}}}
{"type":"event","id":"weekly-chess@…","summary":"Chess Club","start":"2026-09-08T18:00:00","tz":"America/Chicago","duration":"PT2H"}
```

- A JSON array and JSONL carry exactly the same records, and the host picks one per consumer.
- Each line is validated as it arrives against the `$def` its `type` names. Nothing waits for the end of the stream.
- Nushell streams lists natively, so `pipe get` can pass a stream along without buffering it. The web server relays the same lines as Server-Sent Events.
- Live sources (departures, alerts, now-playing) may need `{"type": "remove", "id": …}` or replacement records. The line format leaves room for that.

---

## 2. Shared definitions (`common.schema.json`)

| `$def` | Shape | Notes / maps to |
|---|---|---|
| `record` | `{type, id, source?, partial?, href?, extra?}` | base of every record type, merged in with `allOf` |
| `text` | non-empty string | |
| `content` | `{type: text\|html\|markdown, value, lang?}` | Atom content; JSON Feed `content_html`/`content_text` |
| `date`, `instant`, `local-date-time`, `when` | §0.2 | |
| `tz` | IANA zone name | |
| `duration` | ISO 8601 duration string | |
| `rrule` | RFC 5545 RRULE string | |
| `url` | http(s) URL string | |
| `email`, `phone` | `{address, kind?}`, `{number, kind?}`, where kind is `work\|home\|mobile\|fax` | vCard EMAIL/TEL |
| `geo` | `{lat, lon}` | |
| `address` | `{lines[], locality, region, postcode, country}` | vCard ADR, schema.org PostalAddress |
| `place` | `{id, name, address?, geo?, url?, phones[], building?, room?}` | schema.org Place; GTFS stops and campus buildings are places |
| `person` | `{id, name: {display, given?, family?}, emails[], phones[], titles[], affiliations[]}` | §3.5; vCard, schema.org Person |
| `org` | `{id, name, short_name?, parent?, emails[], phones[], url?}` | vCard KIND=org, schema.org Organization |
| `agent` | `{person: {…}}` or `{org: {…}}` | organizers, authors, employers |
| `link` | `{url, rel, title?, type?}` | `rel` is an IANA link relation or one of ours (`tickets`, `apply`, `boxscore`, `social`, `broadcast`) |
| `image` | `{url, alt?, width?, height?, caption?, credit?}` | |
| `money` | `{min, max?, currency, per?, note?}` or `{free: true}` | `per` is `hour\|week\|semester\|item`, for pay and dues |
| `quantity` | `{value, unit}` (UCUM-ish) | nutrition, distances, track marks |
| `hours` | `{seasons[], exceptions[], areas[]}` | §3.4; buildings, dining, office hours |
| `external-id` | `{id, scheme}`, where scheme is `gtfs\|sidearm\|banner\|orcid\|…` | cross-references the source system without making it the identity |
| `categories`, `keywords` | string arrays | |
| `created`, `modified` | `instant` | |
| `status` | per-domain enums over a shared core: `active`, `cancelled`, `postponed`, `tentative` | |
| `item` | §1.7 | |
| `envelope` | §1.2; host only | |

---

## 3. Domains

Each sketch shows records as a pipe would emit them, followed by the formats each domain should round-trip with and its hard parts.

### 3.1 Feeds (`entry`)

```json
{ "type": "entry", "id": "tag:…,2026:post-123", "source": "https://…/feed",
  "feed": { "title": "Manitou Messenger", "url": "https://…" },
  "title": "…",
  "links": [{ "url": "https://…", "rel": "alternate" }],
  "published": "2026-10-16T08:00:00-05:00", "updated": "2026-10-16T09:12:00-05:00",
  "authors": [{ "person": { "id": "jdoe", "name": { "display": "Jane Doe" } } }],
  "summary": "…",
  "content": { "type": "html", "value": "<p>…</p>" },
  "enclosures": [{ "url": "https://…/ep12.mp3", "type": "audio/mpeg", "length": 12345678, "duration": "PT31M" }],
  "categories": ["News"],
  "image": { "url": "https://…", "alt": "…" } }
```

- **Maps to:** RSS 2.0, Atom (RFC 4287) and JSON Feed 1.1, plus the iTunes/Podcasting 2.0 namespaces for enclosures and Media RSS for images.
- **Hard parts:**
  - RSS dates are RFC 822 and often malformed.
  - RSS has no reliable id, so fall back to the link, then a hash.
  - HTML in `description` vs. `content:encoded`.
  - Podcast fields (season, episode, transcript, chapters) belong under `extra` or a `podcast` object.
- Newspaper issues (§3.12) contain `entry` objects, so building an issue from a feed is a grouping pipe, not a conversion.
- On-demand media (recordings, podcast episodes) are entries with enclosures.

### 3.2 Calendars (`event`)

The two records in §0.1 show the shape, and §7 has the full field list.

- **Maps to:** iCalendar VEVENT (RFC 5545 and 7986), jCal, schema.org Event, and The Events Calendar's WordPress REST feed.
- **Scheduled streams** (a livestreamed concert, a weekly radio show) are events with `"attendance": "online"` or `"hybrid"`, `"online": {"url": …, "platform": …}`, and a `broadcast` link where useful.
- Athletics `game`/`meet` (§3.6) are built from `event`. Org meetings (§3.8) carry an rrule and can be projected into events.
- Gaps compared with Apple Calendar (see `kdl-calendar-schema/ics-vs-apple.md`): busy/free, and attendee kind.

### 3.3 Bus timetables (`trip`, `departure`)

People mean two different things by "timetable", and pipes will want both:

```json
{ "type": "trip", "id": "m-0705", "source": "https://…/gtfs.zip", "tz": "America/Chicago",
  "agency": { "id": "nfld-transit", "name": "Northfield Transit" },
  "route": { "id": "metro", "short_name": "M", "long_name": "Metro Express", "color": "#e5a823", "mode": "bus" },
  "service": { "id": "weekday", "days": ["mo", "tu", "we", "th", "fr"], "start": "2026-09-01", "end": "2026-12-18",
               "exceptions": [{ "date": "2026-11-26", "change": "removed" }] },
  "direction": "outbound", "headsign": "Twin Cities",
  "stop_times": [
    { "stop": { "id": "boe", "name": "Boe Chapel", "geo": { "lat": 44.4614, "lon": -93.1822 } }, "time": "07:05" },
    { "stop": { "id": "cub", "name": "Carleton Union" }, "time": "07:12" },
    { "stop": { "id": "msp", "name": "MSP Airport" }, "time": "08:20" },
    { "stop": { "id": "boe", "name": "Boe Chapel" }, "time": "25:10" } ] }
```

```json
{ "type": "departure", "id": "boe:m-0705:2026-10-12", "tz": "America/Chicago",
  "stop": { "id": "boe", "name": "Boe Chapel" },
  "route": { "id": "metro", "short_name": "M" }, "headsign": "Twin Cities",
  "scheduled": "2026-10-12T07:05:00", "expected": "2026-10-12T07:08:00", "status": "delayed" }
```

- **Maps to:** GTFS static (agency, routes, trips, stops, stop_times, calendar, calendar_dates, frequencies) and, for departures, GTFS-Realtime. Campus shuttle vendors (TransLoc, Passio) expose similar shapes.
- **Hard parts:**
  - Times like `"25:10"` are GTFS-style: past midnight of the service day.
  - Humans read a **stops × trips grid**, but the data is a list of trips. Clients draw the grid, which needs a stop order per route and direction, either a `pattern` on the route or derived from the longest trip.
  - Frequency-based service ("every 20 minutes") is a trip with `"frequency": {"start", "end", "every"}` instead of exact times.
  - Scheduled vs. realtime: `departure` records are separate, so a static timetable never goes stale.
  - GTFS is relational, and embedding repeats route, service and stops in every trip. That's the accepted cost. A pipe that only needs routes or stops can emit those as their own record types.

### 3.4 Building hours (`hours`)

```json
{ "type": "hours", "id": "rolvaag", "tz": "America/Chicago",
  "place": { "id": "rolvaag", "name": "Rolvaag Library" },
  "seasons": [
    { "name": "Fall semester", "start": "2026-09-01", "end": "2026-12-18",
      "open": [
        { "days": ["mo", "tu", "we", "th"], "from": "07:45", "to": "02:00" },
        { "days": ["fr"], "from": "07:45", "to": "21:00" },
        { "days": ["sa"], "from": "10:00", "to": "18:00" },
        { "days": ["su"], "from": "12:00", "to": "02:00" } ] },
    { "name": "Interim", "start": "2027-01-04", "end": "2027-01-29", "open": [ … ] } ],
  "exceptions": [
    { "start": "2026-11-26", "end": "2026-11-27", "closed": true, "note": "Thanksgiving" },
    { "start": "2026-12-11", "open": [{ "from": "07:45", "to": "24:00" }], "note": "Finals" } ],
  "areas": [{ "name": "Help desk", "seasons": [ … ] }],
  "osm": "Mo-Th 07:45-02:00; …" }
```

- **Maps to:** OSM `opening_hours` (the raw string is optionally kept in `osm`), schema.org `OpeningHoursSpecification`, and Google Business Profile hours.
- **Hard parts:**
  - Overnight spans: a `to` earlier than `from` closes after midnight.
  - Seasons tied to the academic calendar, plus holiday and finals exceptions.
  - "24 hours", "by appointment", and "card access only after 22:00" (an `access` field on a span).
  - Sub-areas, such as a desk inside a building.
  - The operator everyone wants is **"open now?"**, which needs the full exception logic, so this shape should be easy to evaluate.

### 3.5 Contacts and directory results (`person`, `org`)

```json
[
  { "type": "person", "id": "jdoe", "source": "https://…/directory?q=registrar",
    "name": { "display": "Jane Doe", "given": "Jane", "family": "Doe" }, "pronouns": "she/her",
    "titles": [{ "title": "Associate Registrar", "org": { "id": "registrar", "name": "Office of the Registrar" } }],
    "emails": [{ "address": "doej@example.edu", "kind": "work" }],
    "phones": [{ "number": "507-786-0000", "kind": "work" }],
    "office": { "id": "tomson", "name": "Tomson Hall", "room": "166" },
    "office_hours": { "open": [{ "days": ["mo", "we", "fr"], "from": "13:00", "to": "15:00" }] },
    "tz": "America/Chicago",
    "photo": { "url": "https://…", "alt": "Jane Doe" },
    "affiliations": [{ "kind": "staff" }] },
  { "type": "person", "id": "s123", "partial": true, "href": "https://…/people/s123",
    "name": { "display": "Sam Student" },
    "affiliations": [{ "kind": "student", "class_year": 2028, "major": "Mathematics" }],
    "mailbox": "SPO 1234" },
  { "type": "org", "id": "registrar", "name": "Office of the Registrar",
    "emails": [{ "address": "registrar@example.edu" }],
    "parent": { "id": "academic-affairs", "name": "Academic Affairs" } }
]
```

- **Maps to:** vCard 4 (RFC 6350), jCard (RFC 7095), schema.org Person/Organization, and LDAP `inetOrgPerson` + `eduPerson` for campus directories.
- **Hard parts:**
  - **People vs. offices:** "Registrar" is a contact but not a person, so both `person` and `org` records can appear in results.
  - Several appointments per person, like faculty in two departments or staff who are also alumni: `titles` is an array, and each entry has its own `org`.
  - Students and staff have different fields. Those go under `affiliations` rather than in separate record types.
  - Privacy: FERPA-suppressed students appear as partial records or not at all. `"restricted": true` lets a client say "contact the Registrar".
  - Name order and name parts vary across cultures. `name.display` is always present, and the parts are optional.

### 3.6 Athletics (`game`, `meet`, `athlete`, `standing`)

```json
{ "type": "game", "id": "2026-10-11-wsoc-carleton", "source": "https://…/schedule",
  "sport": "soccer", "season": "2026", "conference": true,
  "start": "2026-10-11T13:00:00", "tz": "America/Chicago",
  "location": { "place": { "id": "klein-field", "name": "Klein Field" } },
  "sides": [
    { "side": "home", "team": { "id": "stolaf-wsoc", "name": "St. Olaf Oles", "image": { "url": "https://…/logo.png", "alt": "Oles" } },
      "score": 2, "result": "win" },
    { "side": "away", "team": { "id": "carleton-wsoc", "name": "Carleton Knights" }, "score": 1, "result": "loss" } ],
  "periods": [{ "home": 1, "away": 0 }, { "home": 1, "away": 1 }],
  "status": "final",
  "links": [{ "url": "https://…/boxscore", "rel": "boxscore" }] }
```

```json
{ "type": "meet", "id": "2026-10-17-mn-intercollegiate", "sport": "cross-country",
  "start": "2026-10-17T10:00:00", "tz": "America/Chicago", "location": { … },
  "team_results": [{ "team": { "id": "stolaf-mxc", "name": "St. Olaf" }, "place": 2, "points": 67 }],
  "results": [{ "athlete": { "id": "athlete-12", "name": { "display": "…" } }, "place": 5,
                "mark": { "value": "25:31.4", "unit": "time" } }] }
```

- **Maps to:** schema.org SportsEvent/SportsTeam, SIDEARM Sports schedule and results feeds (which most US college athletics sites run on, and which also publish .ics), and ESPN-style JSON.
- `game` and `meet` are built from `event` with `allOf`, so they have start, location, status and links, and project to calendars for free. Results stay out of calendars because a meet can have hundreds of placings.
- **Hard parts:**
  - **Head-to-head vs. meets.** Games have two sides and a score. Track, swimming, cross country, golf and wrestling have many competitors with places and marks (times, distances, points).
  - Sport-specific periods: halves, quarters, innings, sets (volleyball and tennis scores are per set, not cumulative), and overtime or shootouts.
  - Results arrive after the schedule. The `id` stays the same while `status` moves from `scheduled` to `in-progress` to `final`, `postponed` or `cancelled`.
  - Season records (overall `9-3-1`, conference `5-1-0`) are `standing` records. Rosters are `athlete` records: a `person` plus number, position, class year and hometown.

### 3.7 Dictionary (`word`)

The nesting follows Wiktionary and LMF: word, then etymology group, then part of speech, then senses (recursive).

```json
{ "type": "word", "id": "en:bank", "source": "https://en.wiktionary.org/wiki/bank",
  "headword": "bank", "lang": "en",
  "pronunciations": [{ "ipa": "/bæŋk/", "region": "us", "audio": "https://…/bank.ogg" }],
  "etymologies": [
    { "n": 1, "origin": "From Middle English banke, from Old Norse…",
      "pos": [
        { "pos": "noun",
          "forms": [{ "form": "banks", "kind": "plural" }],
          "senses": [
            { "id": "bank-n-1",
              "definition": "An institution where one can place and borrow money.",
              "labels": [{ "label": "finance", "kind": "domain" }],
              "examples": [{ "text": "I need to go to the bank." }],
              "synonyms": ["…"],
              "senses": [
                { "id": "bank-n-1a", "definition": "A building where such an institution operates." } ] } ] },
        { "pos": "verb", "senses": [ … ] } ] },
    { "n": 2, "origin": "From Old Norse bakki…",
      "pos": [
        { "pos": "noun",
          "senses": [
            { "id": "bank-n-2-1",
              "definition": [ { "text": "The edge of a " }, { "xref": "river", "sense": "river-n-1" }, { "text": " or lake." } ],
              "examples": [{ "text": "We picnicked on the bank.", "source": "…" }] } ] } ] } ],
  "related": [{ "term": "embankment", "rel": "derived" }],
  "translations": [{ "term": "Bank", "lang": "de", "sense": "bank-n-1" }] }
```

- **Maps to:** Wiktionary (via wiktextract JSON), Free Dictionary API JSON, WordNet (synsets roughly match senses), OntoLex-Lemon, TEI Lex-0, and LMF (ISO 24613).
- **Hard parts:**
  - **Homographs vs. polysemy:** "bank" (money) and "bank" (river) are different etymologies, not senses of one word. Wiktionary splits by etymology first, while Merriam-Webster splits by homograph number. `etymologies` is optional, so dictionaries without etymology groups put `pos` directly on the word.
  - **Recursive senses** (1, 1a, 1a(i)): a sense contains `senses`. Display numbering is derived, not stored. Each sense has a stable `id` so synonyms, translations and cross-references can point at it.
  - **Labels** (archaic, slang, US, finance) mix register, region and domain, so each label has an optional `kind` of `register|region|domain|grammar`.
  - **Cross-references inside definitions** use the array-of-runs form (§1.4).
  - Some relations are per sense (synonyms) and some are per word (derived terms).
  - Pronunciation varies by region and sometimes by part of speech ("record" as a noun vs. a verb), so it's allowed on the word or on a `pos`.
  - Multilingual dictionaries: `lang` appears on the word, on translations and on examples.

### 3.8 Student orgs (`student-org`)

```json
{ "type": "student-org", "id": "ksto", "source": "https://…/engage", "status": "active",
  "name": "KSTO Radio", "short_name": "KSTO",
  "categories": ["Media"],
  "content": { "type": "html", "value": "<p>…</p>" },
  "image": { "url": "https://…/logo.png", "alt": "KSTO logo" },
  "officers": [{ "person": { "id": "…", "name": { "display": "…" } }, "role": "Station Manager", "term": "2026-27" }],
  "advisors": [{ "person": { … } }],
  "emails": [{ "address": "ksto@example.edu" }],
  "links": [{ "url": "https://instagram.com/…", "rel": "social", "platform": "instagram" }],
  "meetings": [{ "rrule": "FREQ=WEEKLY;BYDAY=WE", "start": "19:00", "tz": "America/Chicago",
                 "place": { "id": "buntrock", "name": "Buntrock Commons" }, "room": "143" }],
  "membership": "open" }
```

- **Maps to:** Anthology (Campus Labs) Engage, CampusGroups and Presence exports, and schema.org Organization.
- `student-org` is built from `org` with `allOf`.
- **Hard parts:** officers are a role, a person and a term, since positions turn over yearly. Meetings are recurring and can be projected into events. Orgs can be chartered, active or inactive. Social links use `rel: social` plus a platform.

### 3.9 Student jobs (`job`)

```json
{ "type": "job", "id": "1234", "source": "https://…", "status": "open",
  "title": "Library Desk Assistant",
  "employer": { "org": { "id": "library", "name": "Rolvaag Library" } },
  "supervisor": { "person": { … } },
  "pay": { "min": 11.50, "max": 14.00, "currency": "USD", "per": "hour" },
  "hours_per_week": { "min": 6, "max": 10 },
  "positions": 4,
  "eligibility": { "work_study": "either", "international": true },
  "posted": "2026-09-01", "closes": "2026-09-15",
  "start": "2026-09-20", "end": "2027-05-20",
  "location": { "place": { "id": "rolvaag", "name": "Rolvaag Library" } }, "remote": "no",
  "content": { "type": "html", "value": "<p>Duties…</p>" },
  "qualifications": ["…"],
  "links": [{ "url": "https://…", "rel": "apply" }] }
```

- **Maps to:** schema.org JobPosting (which Google for Jobs reads), Handshake, and campus HR systems (Workday Student, Banner).
- **Hard parts:** pay ranges and units (`money` with `per`); work-study eligibility (work-study only, not required, or either); rolling deadlines vs. fixed dates; term-based start and end dates; multiple openings.

### 3.10 Course catalog (`course`, `offering`)

A course (the catalog entry) and an offering (a section in a term) are different records. An offering embeds a summary of its course.

```json
[
  { "type": "course", "id": "CSCI-251", "source": "https://…/catalog",
    "subject": "CSCI", "number": "251",
    "title": "Software Design and Implementation",
    "credits": 1,
    "content": { "type": "text", "value": "…" },
    "prereq": { "any": [{ "course": "CSCI-121" }, { "course": "CSCI-125" }] },
    "prereq_text": "CSCI 121 or 125",
    "attributes": [{ "code": "WRI", "scheme": "gen-ed" }],
    "cross_lists": ["…"],
    "offered": ["fall", "spring"] },
  { "type": "offering", "id": "2026-fall:CSCI-251-A", "status": "open",
    "course": { "id": "CSCI-251", "title": "Software Design and Implementation", "credits": 1, "partial": true },
    "term": { "id": "2026-fall", "name": "Fall 2026", "start": "2026-09-08", "end": "2026-12-18" },
    "section": "A",
    "instructors": [{ "person": { … } }],
    "tz": "America/Chicago",
    "meetings": [
      { "days": ["mo", "we", "fr"], "start": "09:05", "end": "10:00", "place": { "id": "rns", "name": "Regents Hall" }, "room": "204" },
      { "days": ["th"], "start": "13:00", "end": "15:00", "kind": "lab" } ],
    "enrollment": { "enrolled": 22, "capacity": 24, "waitlist": 0 } }
]
```

- **Maps to:** schema.org Course/CourseInstance, XCRI-CAP (UK course marketing), 1EdTech (IMS) LIS/OneRoster for sections, and Ellucian Banner/Colleague exports.
- **Hard parts:**
  - **Prerequisites are boolean trees** (`all`, `any`, `course`, `placement`, `standing`, `permission`) and are often only available as prose. Structured prerequisites are allowed, and `prereq_text` keeps the original.
  - Credits can vary; `credits` may be a range like `{ "min": 0.25, "max": 1 }`.
  - Meeting patterns include labs, discussion sections, and meetings limited to part of a term (such as the first half).
  - Cross-listings: one offering under several course ids.
  - Gen-ed and requirement codes vary by school, so each attribute carries a `scheme`.

### 3.11 A–Z links (`bookmark`)

```json
{ "type": "bookmark", "id": "finaid", "source": "https://…/az",
  "url": "https://…/finaid",
  "title": "Student Financial Services",
  "aliases": ["Financial Aid", "Scholarships"],
  "description": "…",
  "categories": ["Students"],
  "owner": { "org": { "id": "sfs", "name": "Student Financial Services" } } }
```

- **Maps to:** schema.org ItemList/SiteNavigationElement, OPML, Netscape bookmarks HTML, and XML sitemaps.
- **Hard parts:** aliases create "see also" entries under other letters ("Financial Aid, see Student Financial Services"). Sorting ignores leading articles and needs Unicode collation. The **letter grouping is derived by the client, not stored**.

### 3.12 Newspaper issues (`issue`)

An issue record holds a whole issue at once (front page, sections, every story), so a client can show it as a unit. A feed, by contrast, gives one article at a time.

```json
{ "type": "issue", "id": "messenger-2026-10-16", "source": "https://…",
  "publication": { "id": "manitou-messenger", "name": "Manitou Messenger", "url": "https://…",
                   "image": { "url": "https://…/masthead.svg", "alt": "Manitou Messenger" } },
  "volume": 139, "number": 6, "date": "2026-10-16",
  "cover": { "url": "https://…/cover.jpg", "alt": "…" },
  "sections": [
    { "id": "news", "name": "News", "order": 1,
      "entries": [
        { "type": "entry", "id": "2026-10-16-tuition", "priority": 1, "page": 1,
          "title": "Board approves tuition freeze",
          "deck": "First freeze in a decade; housing rates still rise 3%",
          "bylines": [{ "person": { "id": "jdoe", "name": { "display": "Jane Doe" } }, "role": "writer" },
                      { "person": { "id": "asmith", "name": { "display": "Alex Smith" } }, "role": "photographer" }],
          "dateline": "NORTHFIELD, Minn.",
          "published": "2026-10-16T08:00:00-05:00",
          "content": { "type": "html", "value": "<p>…</p>" },
          "images": [{ "url": "https://…", "alt": "…", "caption": "Regents met Friday.", "credit": "Alex Smith" }],
          "pull_quotes": [{ "text": "We heard students.", "attribution": "President …" }],
          "continues": { "page": 4 },
          "related": [{ "id": "2026-10-09-budget", "title": "…" }],
          "corrections": [{ "date": "2026-10-17", "text": "An earlier version misstated the housing increase." }] } ] },
    { "id": "opinion", "name": "Opinion", "order": 2, "entries": [ … ] } ],
  "staff": [{ "person": { … }, "role": "Editor-in-Chief" }] }
```

- **Maps to:** schema.org PublicationIssue, PublicationVolume and NewsArticle; IPTC NewsML-G2 and NITF (what wire services and many CMSes use); WordPress issue plugins. Issue PDFs on Issuu are the usual fallback source.
- **Hard parts:**
  - **Order and prominence.** Showing an issue needs section order, then article order and weight within each section (lead story, secondary stories, briefs). Sections have `order`, entries have `priority`, and `page` appears only when there's a print edition.
  - **Bylines with roles** (writer, photographer, illustrator, graphics), often several per article.
  - The articles are `entry` records (§3.1) with print fields added (`deck`, `dateline`, `page`, `continues`, `pull_quotes`), so a pipe can build an issue by grouping a week's feed entries by category. Many student papers only publish dated posts online, so print fields are never required.
  - Corrections and updates after publication; jumps ("continued on page 4"); recurring columns. Ads are out of scope, or go under `extra`.

### 3.13 Radio stations (`station`, `now-playing`, `spin`)

A `station` record has everything a player needs. The DJ schedule lives inside it as recurring shows, and `now_playing` says how to find out what's on right now.

```json
{ "type": "station", "id": "ksto", "source": "https://…",
  "name": "KSTO 93.1", "call_sign": "KSTO", "tz": "America/Chicago",
  "org": { "id": "ksto", "name": "KSTO Radio" },
  "image": { "url": "https://…/logo.png", "alt": "KSTO" },
  "streams": [
    { "url": "https://stream.example.edu/ksto.mp3", "type": "audio/mpeg", "bitrate": 128 },
    { "url": "https://stream.example.edu/ksto.aac", "type": "audio/aac", "bitrate": 64 },
    { "url": "https://stream.example.edu/ksto.m3u", "type": "audio/x-mpegurl", "playlist": true } ],
  "broadcast": [{ "band": "fm", "frequency": 93.1, "unit": "MHz" }],
  "phones": [{ "number": "507-786-3602", "kind": "request-line" }],
  "links": [
    { "url": "https://…/listen", "rel": "player", "title": "Web player" },
    { "url": "https://discord.gg/…", "rel": "chat", "platform": "discord" },
    { "url": "https://…", "rel": "alternate" } ],
  "now_playing": [
    { "method": "icecast-status", "url": "https://stream.example.edu/status-json.xsl", "mount": "/ksto.mp3", "field": "title" },
    { "method": "icy" },
    { "method": "schedule" } ],
  "schedule": [
    { "id": "morning-mix", "name": "The Morning Mix", "rrule": "FREQ=WEEKLY;BYDAY=MO,WE,FR",
      "start": "08:00", "duration": "PT2H", "term": { "start": "2026-09-08", "end": "2026-12-18" },
      "hosts": [{ "person": { "id": "dj-ana", "name": { "display": "DJ Ana" } } }],
      "genres": ["indie"], "content": { "type": "text", "value": "…" } } ] }
```

```json
{ "type": "now-playing", "id": "ksto:2026-10-12T20:14:03Z", "station": { "id": "ksto", "name": "KSTO 93.1" },
  "at": "2026-10-12T20:14:03Z",
  "show": { "id": "evening-static", "name": "Evening Static" },
  "track": { "title": "…", "artist": "…", "album": "…" },
  "listeners": 42 }
```

- **What already exists:** nothing covers the whole player, but each piece has a standard:
  - **RadioDNS / Hybrid Radio SPI** (ETSI TS 102 818) is the closest: XML Service and Programme Information with bearers (FM, DAB, IP streams), logos, links and a programme schedule. It's built for broadcasters and car radios, though, and has no chat, request line or now-playing method.
  - **schema.org RadioStation / BroadcastService / BroadcastEvent** covers the station and its schedule at a coarse level.
  - **Radio Browser** (radio-browser.info) has a community station-directory JSON (`url`, `url_resolved`, `homepage`, `favicon`, `codec`, `bitrate`, `tags`), which is good for the `streams` part and for importing stations.
  - **Spinitron** is what many US college stations use for DJ schedules and playlists. Its API has shows, personas (DJs) and spins, which map to `schedule`, `hosts` and `spin` records.
  - For now-playing: **Icecast** `status-json.xsl`, **Shoutcast** `/stats?json=1`, in-stream **ICY** metadata (`StreamTitle`), and **AzuraCast**'s now-playing JSON.
  - Stream URLs are often **PLS/M3U** playlists that need resolving to the real stream.
- **Hard parts:**
  - **"How to find the current show"** is a procedure, not a value. `now_playing` is an ordered list of methods (`icecast-status`, `shoutcast`, `icy`, `azuracast`, `spinitron`, `json` with a URL and a JSON Pointer, and `schedule` as the fallback) so a player tries each in turn. The `now-playing` record is what a pipe produces after doing that work, so it streams naturally (§1.8).
  - Schedules are term-based at college stations and change every semester. Each show is an rrule plus a `term` window, and one-off specials are ordinary `event` records.
  - The "show name" in stream metadata is often actually the track title. `now-playing` keeps `show` and `track` separate, and the schedule fills in the show when the stream only reports tracks.
  - Playlists: a `spin` record (track, artist, album, label, time, show) per song played, from Spinitron or a station's own log.

---

## 4. More domains

These are listed roughly by how often a campus or Pipes-style app would want them.

| Domain | Why | Maps to |
|---|---|---|
| **Dining menus** | The most-checked campus data after the shuttle: meal periods × stations × items, with dietary tags, allergens and nutrition | Bon Appétit / Nutrislice / Sodexo menus; schema.org Menu |
| **Places / campus map** | Buildings, rooms, parking: `place` as a record type | GeoJSON, OSM, schema.org Place |
| **Alerts & notices** | Emergency alerts, closures, service outages; short-lived and ranked by severity | CAP (Common Alerting Protocol), statuspage APIs, RSS |
| **Live availability** | Laundry machines, open lab computers, parking spaces, printers: counts with a timestamp | vendor APIs; there's no standard, which is why a schema helps |
| **Academic calendar / terms** | Term dates, breaks and deadlines; feeds hours seasons, catalog terms and job dates | iCalendar (events plus a `term` record) |
| **Media playlists** | Video and podcast playlists beyond feeds | XSPF, M3U |
| **Library / bibliographic** | Book search, holdings, citations | CSL-JSON, BibTeX, MARC, BIBFRAME |
| **Weather** | A classic Pipes input | NWS API, OpenWeather |
| **FAQ / knowledge base** | Q&A pairs, often scraped | schema.org FAQPage |
| **Generic table** | An escape hatch when nothing fits: `row` records with typed values, so any nushell table can be emitted | CSV, Frictionless Table Schema |

The **generic table** is worth doing early. It's what a pipe emits before anyone writes a schema for its data, and it gives clients a default.

---

## 5. Build order

1. `common.schema.json`, the validator (§6.5), and the calendar schema (§7).
2. `table`, `feed` (`entry`) and `item`, then `issue` on top of `entry`.
3. `directory`, `hours` and `links` (small, and they reuse shared definitions heavily).
4. `timetable`, `catalog`, `jobs`, `orgs`, `radio` and `dining`.
5. `athletics` and `dictionary` last, since they need the most domain modeling.

---

## 6. Pipes

A pipe is a nushell module whose `main` returns records, e.g.:

```nu
http get "https://…/ical.cfm" | from ics | where start > (date now)
```

```nu
let first = pipe get "first"
let second = pipe get "second"
$first | append $second
```

Whatever `main` returns is validated against the pipe's declared record types, so downstream code can trust it: timestamps are well formed and required fields exist.

Nushell fits well here. Its values are already structured (records, tables, and typed `datetime`, `duration` and `filesize`). `from ics`, `from json` and `from xml` either exist or are easy to add (`nushell-ics/ics.nu` is one). And nearly every Yahoo Pipes operator (fetch, filter, sort, union, truncate, unique, rename, regex) is a single nushell command.

### 6.1 Pipe files

A pipe is a `.nu` module, so it gets real tooling (nu LSP, syntax highlighting, `nu` can run it, error line numbers match the file) and parameters come straight from the signature. Here's a sample, also at [`pipes/campus-week.nu`](pipes/campus-week.nu):

```nu
export const pipe = {
    output: ["https://pipes.dev/stdlib/calendar/1#event"]
    refresh: 1hr
}

# This week on campus
#
# Upcoming events from the St. Olaf calendar.
@category pipes
@search-terms calendar events
@example "Next three days" { campus-week --days 3 }
export def main [
    --days: int = 7                    # How far ahead to look
    --tz: string = "America/Chicago"   # Time zone for floating times
]: nothing -> table {
    http get "https://www.stolaf.edu/apps/calendar/ical.cfm"
    | from ics
    | where start < ((date now) + ($days * 1day))
    | into event --tz $tz
}
```

The host loads a pipe with `use`, which evaluates consts but doesn't run `main`. Then it reads (checked on nushell 0.116.1):

| What the host needs | Where it comes from | How the host reads it |
|---|---|---|
| Title, description | `main`'s doc comment (first line, then the rest) | `scope commands` → `description`, `extra_description` |
| Params: name, type, default, help text | `main`'s signature and trailing comments | `scope commands` → `signatures` (`parameter_name`, `syntax_shape`, `parameter_default`, `description`) |
| Category, search terms, examples | `@category`, `@search-terms`, `@example` | `category`, `search_terms`, `examples` |
| Output record types, refresh interval, license | `export const pipe = {…}` | `$campus_week.pipe` |

Custom attributes such as `@output "…"` aren't used, because an attribute must be a command that can run at parse time and a nushell `def` can't (0.116.1 reports `nu::shell::not_a_const_command`). The host could register them as builtins, but then stock `nu pipe.nu` would fail to parse. `export const pipe` works everywhere.

Also:
- A pipe's parameters are `main`'s flags, and the UI draws a form from `signatures`.
- `pipe get "<name>"` is a host command, not `use other-pipe.nu`, so another pipe's output still goes through the cache, sandbox and validation and arrives already validated. The host detects cycles.
- Helper modules (`stdlib/ics.nu` and the `into <type>` helpers) are imported with an ordinary `use`.

### 6.2 Normalizing records

Nushell records map onto JSON objects directly. What's left is normalization, which each record type's helper, `into <type>` (`into event`, `into entry`, …), handles:

- It sets `type` on each record.
- It formats datetimes per field (§6.3).
- It renames common source columns to schema names where the mapping is unambiguous. Most of `ics.nu`'s columns already match.
- It drops nulls, so optional fields are absent rather than `null`.

The reverse direction, reading records into a plain table, needs nothing extra. `to-item` (§1.7) ships alongside.

### 6.3 Time zones

A nushell `datetime` always has an offset, and `to json` writes it with that offset. That's right for `instant` fields but wrong for floating local times (`local-date-time` plus `tz`) and all-day dates (§0.2). So the `into <type>` helpers format time fields explicitly from the schema:
- date-only fields as `YYYY-MM-DD`
- local fields as `YYYY-MM-DDTHH:MM:SS`, plus the record's `tz`
- instants as RFC 3339 with `Z`

They take a `--tz` flag rather than trusting offsets.

### 6.4 Sandboxing

A shared pipe is someone else's code. The host embeds nushell (the Rust `nu-engine`) with a hand-picked command set:

- no external commands (`^cmd`, `run-external`), no filesystem writes, no `cd`, and no plugins except ours
- network access only through our `http get`, which caches, rate-limits, and records each fetch for the run log
- time and memory limits per run

The CLI can offer an unsafe mode for running your own pipes against local files.

### 6.5 Validation

Validation happens once, at the pipe boundary: after `main` returns and before anything else sees the output, whether that's a consumer or another pipe via `pipe get`. Each record is checked against the `$def` its `type` names, and that type must be one of the pipe's declared `output` types. On failure, the host reports the record index (or JSONL line) and the JSON Pointer within the record, and keeps the last good output for consumers.

The engine is Rust, so the validator is the `jsonschema` crate (2020-12, format assertion on, registry preloaded with the stdlib). It's also exposed to nushell as a `validate` command so pipe authors can check their output while writing. For JavaScript consumers, ajv (2020) plus ajv-formats is an easy equivalent.

---

## 7. Calendar schema

The first schema to build is the calendar. `kdl-calendar-schema/` already has a field set and real examples, including St. Olaf's feed, to build it from.

1. **`common.schema.json`** (`$id: …/common/1`), containing the §2 definitions.
2. **`calendar.schema.json`** (`$id: …/calendar/1`, version 1.0.0), with one record type, `event`. Fields carried over from `calendar.schema.kdl`:

   | `calendar.schema.kdl` | `event` record |
   |---|---|
   | `calendar` name, `tz`, `color`, `source` | pipe doc comment; each event's `tz` and `source`; `color` dropped (a client concern) |
   | `venue <id> { … }` + `location venue=<id>` | `location.place` embedded `place` |
   | `event "<uid>" featured= hidden=` | `id`, `featured`, `hidden` |
   | `start … tz= all-day=`, `end … tz=` | `start`, `end`, `all_day`, `tz` |
   | `location "text" { geo }` | `location: {name, geo}` |
   | `category`, `keyword` | `categories[]`, `keywords[]` |
   | `link … rel= title=`, `image … alt=` | `links[]`, `images[]` |
   | `cost free` / `cost 5 10 currency=` | `cost: {free: true}` / `cost: {min, max, currency, note}` (`money`) |
   | `organizer`, `attendee` | `organizers[]`, `attendees[]` (embedded `agent` plus role and RSVP for attendees) |
   | `exdate`, `reminder` | `exdates[]`, `reminders[]` |
   | `summary`, `description`, `excerpt`, `url`, `status`, `duration`, `rrule`, `attendance`, `online`, `created`, `modified` | same names |

   An event has a single `tz`. Events that cross time zones (flights) are rare in our sources, and an `end_tz` field can be added if one shows up.
3. **Examples:** convert `kdl-calendar-schema/examples/*.kdl` into `examples/*.json` (and one `.jsonl`), and convert `invalid/broken.kdl` into failing records with one mistake each.
4. **Validator:** the Rust validator from §6.5, with a check that every example passes and every invalid record fails.
5. **`ics.nu`:** add `into event`, then run it end to end on the St. Olaf feed and validate the result.
