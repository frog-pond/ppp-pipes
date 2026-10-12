# ppp-pipes CLI prototype: design

Status: draft for review · 2026-10-11

This spec covers the first build of ppp-pipes: a Rust core that runs nushell pipes, a CLI on top of it, and a nushell plugin that brings the same commands to stock `nu`. It ships one record type, `event`. [`brainstorm.md`](../../../brainstorm.md) remains the reference for record conventions (§0–1), shared definitions (§2) and the other domains (§3–4). This spec says where the prototype follows it and where it departs.

---

## 1. Goals

**Why.** ccc-server proxies and reshapes campus data (calendars, athletics, feeds, menus, orgs, jobs) with hand-written adapters for each domain and school. ppp-pipes replaces those adapters with *pipes*: small nushell modules whose output is checked against shared JSON Schemas. Later the same engine will run inside ccc-server (Node) and inside client apps (iOS and Android), so apps can fetch sources themselves instead of going through ccc-server and its rate limits.

**This prototype** proves the engine and the schema model from the command line, with calendars as the test domain.

**Success means:**

1. `ppp run campus-week` prints validated `event` JSON built from the live St. Olaf feed.
2. Five more acceptance pipes validate against live sources: Carleton ICS, Northfield ICS, St. Olaf's WordPress Tribe API, Presence, and one pipe that combines the others.
3. Recurring events expand correctly, including DST changes, EXDATE, RDATE and RECURRENCE-ID overrides.
4. The same commands work in stock `nu` through `nu_plugin_pipes`, and pipe files run unchanged there.

**Out of scope:** the HTTP server, SSE and JSONL streaming, a scheduler, keeping the last good output, record types other than `event`, a UI, Node and mobile bindings (including checking that the engine builds for iOS and Android), and memory limits per run (§8.2).

---

## 2. Decisions

| Topic | Decision | Why |
|---|---|---|
| Structure | Rust library core plus thin front ends | The core is what ccc-server and the apps will embed |
| Schemas | `common` and `calendar` only | One complete schema tests every convention; more types add little to an engine prototype |
| ICS parsing | Our own `from ics` in Rust, a port of [`nushell-ics/ics.nu`](../../../nushell-ics/ics.nu) | The official `from ics` (`nu_plugin_formats`) returns raw property lists, lives in private modules, and only runs as a separate plugin process, which mobile does not allow |
| Time library | `jiff` for all our own code | Clear time zone model; bundles the tz database |
| chrono | Only at two boundaries: nushell's `Value::Date`, and the `rrule` crate | Both are forced on us; a jiff RRULE expander would be a project of its own |
| Recurrence | Wrap the `rrule` crate (0.14) in one module | Mature, pure Rust; its chrono types do not leak out |
| Time formatting | Driven by the schema, at the boundary (§5.3) | About 25 record types are planned, and the schema already says which fields are dates, local times or instants |
| Command names | `into event`, `expand rrule`, `from ics`, … with no `pipes-` prefix | Nushell modules already namespace (`use ppp` gives `ppp into event`); a test catches clashes with built-ins |
| URL handling | Nushell's own `url parse --base` and `url join` | Already resolves relative URLs and builds queries |
| Stock `nu` support | A plugin, `nu_plugin_pipes`, that links the core | Each command has one implementation and two thin wrappers (§4.3) |

---

## 3. Workspace layout

```
Cargo.toml                     workspace
crates/
  ppp-core/                    library
    src/
      engine.rs                builds the sandboxed EngineState
      pipe.rs                  loads a pipe, reads PipeInfo, runs main
      schema.rs                schema registry, validators, time-field maps
      serialize.rs             nushell Value -> JSON, using time-field maps
      cache.rs                 HTTP cache on disk
      time.rs                  jiff helpers; chrono <-> jiff conversion
      recur.rs                 wraps the rrule crate
      commands/                one module per command (§4.2)
  ppp-cli/                     binary `ppp`
  nu_plugin_pipes/             binary `nu_plugin_pipes`
stdlib/
  common.schema.json
  calendar.schema.json
  ppp/mod.nu                   the `ppp` nushell module (§6)
pipes/                         acceptance pipes (§7)
fixtures/
  ics/                         KDL examples as .ics, saved live feeds, edge cases
  json/                        saved Tribe and Presence responses
  examples/                    valid event records (.json)
  invalid/                     invalid event records, one mistake each
kdl-calendar-schema/           reference: field set and examples
nushell-ics/ics.nu             reference: the parser `from ics` ports
```

`stdlib/` is the single copy of the schemas and the `ppp` module. The binaries embed it with `include_str!`. Stock `nu` users add `stdlib/` to `NU_LIB_DIRS`.

The workspace pins nushell crates to 0.116.1. The plugin protocol requires the plugin and `nu` to match, so upgrading nushell means rebuilding both.

---

## 4. Components

### 4.1 `ppp-core`

- **`engine`** builds a nushell `EngineState` from a hand-picked command list: `nu-cmd-lang`, a chosen subset of `nu-command`, and our commands. It leaves out external commands (`^cmd`, `run-external`), all filesystem commands (`open`, `ls`, `save`, `rm`, …), `cd` and plugins. The engine itself still reads pipe files and `stdlib/` when it resolves `use`. Our `http get` replaces the built-in one. The embedded `stdlib/` is on the module path, so `use ppp *` works. `$env.PWD` is `/`, because some commands (`http get` among them) need a working directory and pipes have no filesystem to work in. The engine installs nushell's default rustls crypto provider so `http get` can use TLS. `nu-command` is built with its `os` feature: other nushell crates turn on `nu-protocol`'s `os` feature, and `nu-command` does not compile unless its own matches.
- **`pipe`** loads a `.nu` file with `use`, which evaluates its consts without running `main`. It then reads a `PipeInfo`:

  | Field | Source |
  |---|---|
  | title, description | `main`'s doc comment, via `scope commands` |
  | params (name, type, default, help) | `main`'s signature |
  | category, search terms, examples | `@category`, `@search-terms`, `@example` |
  | output types, refresh, license | `export const pipe = {…}` |

  It runs `main` with typed arguments and returns the list of records.
- **`schema`** holds a registry of the embedded schemas, keyed by `$id`. For each record type (each `$def` with an `$anchor`) it compiles a `jsonschema` validator (2020-12, format assertion on) and a **time-field map** (§5.3). An unknown `$id` is an error. Nothing is fetched.
- **`serialize`** turns a nushell record into JSON, formatting time fields according to the map.
- **`cache`** stores HTTP responses on disk, as a body file and a JSON metadata file named by the SHA-256 of the URL. A response is fresh for the pipe's `refresh` if it has one, else for the response's `Cache-Control: max-age`, else not at all; a stale one is revalidated with `ETag`/`Last-Modified`. The CLI puts it in `<user cache dir>/ppp-pipes/http` (`dirs` crate). Embedders pass their own path, or none.
- **`host`** holds what every engine of one run shares: the cache directory, the pipes directory `pipe get` reads, the limits, the flag a timeout sets, the stack of running pipes, and warnings for the caller to show.
- **`time`** holds jiff helpers and the only chrono ↔ jiff conversions.
- **`recur`** is the only module that touches the `rrule` crate.

### 4.2 Rust commands

| Command | Input → output | Notes |
|---|---|---|
| `from ics [--tz]` | string → table | §5.2 |
| `expand rrule --from <dt> --until <dt>` | table → table | §5.4 |
| `html to-text` | string → string | Strips tags and decodes entities (`&amp;`, `&#8211;`); crate chosen in the plan |
| `http get <url> [--raw]` | → any | Replaces the built-in: http(s) only, through the cache, with the size cap. JSON (by `Content-Type`) is parsed unless `--raw`; other bodies are text when UTF-8, else binary |
| `pipe get <name> [args]` | → table | §5.5 |
| `pipe warn <message>` | any → any | Reports a warning (stderr under `ppp run`) and passes its input on |
| `query web` | string → any | nushell's own command from `nu_plugin_query`, run in-process (its work is a public function that needs no plugin connection); in stock `nu` it comes from that plugin |
| `validate` | table → table | Returns its input unchanged, or fails with index and pointer |
| `to pipes-json` | table → string | Canonical JSON, with time fields formatted by the schema |

### 4.3 One implementation, two wrappers

Each command is a plain function: typed arguments and a `Value` in, a `Value` or error out. Two wrappers of about ten lines each call it:

- an in-process `nu_protocol::engine::Command`, for the runner and for embedding;
- a `nu_plugin::SimplePluginCommand`, for `nu_plugin_pipes`.

`nu_plugin_formats` has only the second kind, which is why we cannot link it into our own binary.

The function returns a small `CommandError` (title and message), which each wrapper turns into its own error type: a `ShellError` in the engine, a `LabeledError` in the plugin. The plugin's wrappers borrow name, signature and description from the engine's command, so the two can't drift apart.

### 4.4 `ppp-cli`

```
ppp run [--pipes-dir DIR] [--cache-dir DIR | --no-cache] [--timeout SECS] <pipe> [pipe flags…]
ppp info <pipe> [--pipes-dir DIR]
ppp validate <file.json | file.jsonl>
```

- `<pipe>` is a name, looked up as `<pipes-dir>/<name>.nu` (default `./pipes`), or a path.
- `run` passes any remaining arguments to `main` exactly as `nu <pipe>.nu <args>` would: each is quoted as needed (`nu_parser::escape_for_script_arg`) and nushell's parser checks them against `main`'s signature, so every flag type works and errors are nushell's own (unknown flag, type mismatch, missing value). Embedders don't go through this; they pass typed values. `ppp run`'s own options (`--pipes-dir`) go before the pipe name, since everything after it belongs to the pipe.
- `info` prints the `PipeInfo` as JSON: name, title, description, category, search terms, examples, flags (name, type, description, default), output types and refresh (ISO 8601). It doesn't run the pipe. This is the metadata a UI will later turn into a form.
- `validate` checks a JSON array or JSONL file against the stdlib.

### 4.5 `nu_plugin_pipes`

A standard nushell plugin that offers every command in §4.2 except `query web`, which comes from nushell's own `nu_plugin_query`. In stock `nu`:

```nu
plugin add ~/.cargo/bin/nu_plugin_pipes; plugin use pipes
plugin add ~/.cargo/bin/nu_plugin_query; plugin use query  # for pipes that read HTML, such as carleton-week
use ppp *
http get $url | from ics | expand rrule --from (date now) --until ((date now) + 1wk) | into event | validate
pipe get campus-week {within: 3day}
```

Pipe files run unchanged: `use pipes/campus-week.nu; campus-week --within 3day`. Without `nu_plugin_query`, a pipe that uses `query web` fails to load: stock `nu` has a built-in `query` command, so the parser reports "Extra positional argument" rather than an unknown command.

Outside the runner there is no sandbox, and `http get` is nushell's own, without our cache. `pipe get` is the exception: it runs the full sandboxed engine inside the plugin process, with the cache (in the user cache directory), validation and a 60-second limit. In the plugin it takes `--pipes-dir`, relative to nushell's current directory (default `./pipes`).

The plugin is built against nushell 0.116.1 and loads only into that version.

The library module is called `ppp`, not `pipes`, because nushell's `use` looks for a directory of that name in the current directory before `NU_LIB_DIRS`, and `pipes/` is where pipe files conventionally live.

---

## 5. Data flow

### 5.1 `ppp run`

1. Find the pipe file and load it into the sandboxed engine. Read its `PipeInfo`.
2. Match the CLI flags against `PipeInfo.params`.
3. Run `main` and collect the list of records.
4. At the boundary, for each record:
   - its `type` must be one of the pipe's declared `output` types;
   - find the `$def` whose `$anchor` matches `type`;
   - serialize it with that type's time-field map;
   - validate it, collecting `{index, pointer, message}` for each failure.
5. If every record passes, print a JSON array on stdout and exit 0. Otherwise print the failures on stderr and exit 1.

### 5.2 `from ics`

A port of `ics.nu`. It keeps the same columns (`summary`, `start`, `end`, `all_day`, `location`, `description`, `url`, `categories`, `organizer`, `geo`, `uid`, `status`, `rrule`, `created`, `last_modified`, `props`) and adds:

- `tz`: the zone each event's times belong to;
- `rdates`, `exdates`: lists of datetimes;
- `recurrence_id`: set on override VEVENTs, which are returned as their own rows.

It handles CRLF and LF line endings, folded lines, quoted parameters, TEXT escapes, comma-separated lists, `DURATION` in place of `DTEND`, and `VALUE=DATE`. It reads only VEVENTs. VTIMEZONE blocks and their nested RRULEs never become events.

**Zone for each time**, in order: the property's `TZID`; then the calendar's `X-WR-TIMEZONE`, `TIMEZONE-ID`, or the `TZID` of its only VTIMEZONE (St. Olaf's feed names its zone only that way); then `--tz`. A `TZID` that isn't an IANA zone name is an error. A `Z` time is UTC. A floating time with none of these is an **error** that says to pass `--tz`. Unlike `ics.nu`, it never falls back to the machine's local zone, because embedded hosts run wherever the server runs.

All-day dates (`VALUE=DATE`) take the same zone when there is one. With no zone anywhere they are not an error: RFC 5545 dates have no zone, so `from ics` stores them as midnight UTC and gives the record no `tz`.

Bad optional properties, such as Northfield's `DTSTAMP;TZID=…` or an unreadable `CREATED`, are kept in `props` and do not fail the parse. A bad `DTSTART`, `DTEND`, `RDATE`, `EXDATE` or `RECURRENCE-ID` does.

`URL` is a URI, which RFC 5545 doesn't escape, but Carleton's feed escapes it like TEXT (`&amp\;sport_id=`). `from ics` removes TEXT escapes from `URL`, since a URI never contains a backslash.

### 5.3 Time formatting

Inside a pipe, times are nushell `datetime` values, so `where start < (date now)` and `sort-by start` work. At the boundary, `serialize` formats each time field according to its schema definition from brainstorm §0.2:

| Schema definition | Output |
|---|---|
| `instant` | RFC 3339 in UTC: `2026-10-14T00:30:00Z` |
| `date` | `2026-10-14`, taken in the record's `tz`; with no `tz`, in the datetime's own offset |
| `local-date-time` | `2026-10-14T19:30:00`, wall time in the record's `tz` |
| `when` (any of the three) | `date` if the record has `all_day: true`; otherwise `local-date-time` if it has `tz`; otherwise `instant` |

**Time-field map.** When the registry loads, it walks each record type's `$def` and lists the paths of its time fields, such as `start`, `end`, `exdates[]` and `reminders[].at`. The walker understands only the keywords our schemas use: `$ref`, `allOf`, `properties`, `items`, `if`/`then`/`else`. Any other keyword on a path to a time definition is an error at load time, and a test loads every stdlib type.

**Dates without `tz`.** Reading a date in its own offset writes it as the source gave it. The one risk: a pipe that moves such a datetime to another zone (`date to-timezone America/Chicago`) turns midnight UTC into 19:00 the day before, and the date shifts. `into event` documents this.

**DST.** A wall time that falls in a spring-forward gap, or twice in a fall-back hour, resolves with jiff's `compatible` rule, which matches RFC 5545.

### 5.4 `expand rrule`

`expand rrule` works on `from ics` rows, before `into event`.

- `--from` and `--until` are required, because rules can run forever.
- Rows without `rrule` or `rdates` pass through unchanged. That includes override rows (those with `recurrence_id`).
- Each master row becomes one row per occurrence that overlaps `[from, until)`, the same rule `during` uses, so an occurrence already under way counts. An occurrence with no `end` is a moment and must start inside the window. Each occurrence row:
  - has `start` at the occurrence and `end` at the same length after it;
  - has `recurrence_id` set to its original start, so `into event` gives it the id `<uid>_<start as UTC basic format>` (e.g. `abc_20261013T170000Z`) and `series: <uid>`. That is the form Google Calendar uses, so expanded feeds and Google's pre-expanded feeds (Carleton) produce the same ids;
  - has no `rrule`, `rdates` or `exdates`.
- DTSTART always counts as an occurrence, even when the rule would skip it (RFC 5545 §3.8.5.3).
- An occurrence that has an override row in the input (same `uid`, `recurrence_id` equal to the occurrence's start) is left out, since the override row stands in for it. An override that moves an occurrence into or out of the window is handled by whatever filters the rows afterwards, usually `during`.
- Expansion runs in wall time in the row's `tz` (UTC when it has none), so an 18:00 event stays at 18:00 across DST. A wall time that falls in a spring-forward gap moves later by the gap, as RFC 5545 says. All-day events stay all-day.
- `UNTIL` without a `Z` (a DATE for all-day series, a floating time for floating ones, as RFC 5545 requires) is read in the series' zone; a DATE `UNTIL` covers its whole day. An `UNTIL` before `DTSTART` leaves just DTSTART.
- An all-day occurrence lasts the same number of calendar days as its master, even across a DST change.
- It fails after 10,000 occurrences of one series inside the window. Occurrences before the window don't count, so a long-running series is fine.
- It only expands. To filter by window, use `during`.

If a pipe skips `expand rrule`, the master keeps its `rrule`, and `into event` gives each override row the same `<uid>_<instant>` id and `series`. Ids match either way.

### 5.5 `pipe get`

`pipe get <name> [args]` runs another pipe through steps 1–4 of §5.1, in its own engine sharing the run's host (cache, timeout, warnings), and returns its records as nushell values, with datetimes intact. `args` is a record of the other pipe's flags, e.g. `{within: 3day}`. If the other pipe's records fail the check, `pipe get` fails and shows the first five failures.

A run-wide stack of pipe names catches cycles. The error shows the chain: `all-campus → campus-week → all-campus`.

---

## 6. Schemas and the `ppp` module

### 6.1 Schemas

**`common.schema.json`** (`$id: https://pipes.frogpond.tech/stdlib/common/1`) holds the definitions from brainstorm §2 that `event` needs: `record`, `text`, `content`, `date`, `instant`, `local-date-time`, `when`, `tz`, `duration`, `rrule`, `url`, `email`, `phone`, `geo`, `address`, `place`, `person`, `org`, `agent`, `link`, `image`, `money`, `categories`, `keywords`, `created`, `modified`. The other definitions wait until a record type needs them.

**`calendar.schema.json`** (`$id: https://pipes.frogpond.tech/stdlib/calendar/1`, `version: 1.0.0`) defines one record type, `event`, with `$anchor: event`. It takes the fields from brainstorm §7 and adds:

| Field | Type | Meaning |
|---|---|---|
| `rdates` | `when[]` | Extra occurrences |
| `series` | string | The master's `uid`, on expanded occurrences and overrides |
| `recurrence_id` | `when` | Which occurrence an override replaces |

It drops the KDL schema's `color`. Records are closed (`unevaluatedProperties: false`) except for `extra`. A record with a local time must carry `tz` (`if`/`then`).

### 6.2 Three layers of normalization

Turning a source into events happens in three layers, each with one job:

1. **Format readers** turn a vendor format into one shared intermediate table: the `from ics` columns plus optional `description_html`, `excerpt`, `place` (a stdlib place), `organizers` (stdlib agents), `images`, `links`, `cost`, `keywords`, `online`, `attendance`, `featured`, `hidden` and `extra`.
   - `from ics` (Rust)
   - `from tribe-events` (nu): The Events Calendar's WordPress REST API, used by many WordPress sites
   - `from presence` (nu): Presence's events API, used by many campuses
2. **`into event [--source] [--tz]`** turns the intermediate table into `event` records. `--source` sets each record's `source`; `--tz` fills in rows with no `tz`. It does the cleanup every source needs:
   - sets `type: "event"`;
   - renames columns (`uid` → `id`, `location` → `location.name`, `organizer` → `organizers[]`, …);
   - makes an organizer a `person` when its `CUTYPE` is `INDIVIDUAL` and an `org` otherwise, including when `CUTYPE` is missing (campus calendars are mostly run by offices; RFC 5545's default is an individual);
   - keeps only `http(s)` URLs as `url`, and `geo` only alongside a location name; anything else moves to `extra`;
   - lower-cases `status` and keeps it only when it is `tentative`, `confirmed` or `cancelled`;
   - turns `description_html` into `content: {type: html}` plus a plain-text `description` (via `html to-text`);
   - runs `html to-text` on short text fields (`summary`, `location.name`);
   - gives override rows their `<uid>_<instant>` id and `series`;
   - percent-encodes non-ASCII characters in every URL (`url`, `links`, `images`, `online`), as the schema's `uri` format requires;
   - moves leftover columns to `extra`; for `from ics` rows, each unmapped property is kept under its lower-cased name, as a list when it repeats (e.g. several `ATTENDEE`s);
   - drops empty values (`drop-empty`).
3. **The pipe file** fixes one site's quirks, such as Northfield's relative URLs or Carleton's HTML in a TEXT field.

The rule: if a second source would hit a problem, the fix goes in layer 1 or 2. If only one site has it, the fix goes in the pipe.

`during <from> <until>` keeps rows whose `start`–`end` span overlaps the window, so events already under way are kept; a row with no `end` is a moment at `start`. Pipes use it for "what's on this week" windows.

`drop-empty` removes `null`, `""`, `[]` and `{}`. `from tribe-events` treats Tribe's `venue: []` and `image: false` as absent, reads times from its `utc_*` fields (a bare local time would be read in the machine's zone), turns all-day `23:59:59` ends into exclusive next-day ends, and maps country names to ISO codes (US, CA) or drops them.

---

## 7. Acceptance pipes

Each lives in `pipes/`, declares `output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"]`, and takes `--within <duration>` and `--from <datetime>` (default: now) for its window. Those reading one source also take `--source <url>`, defaulting to the live URL, so tests can point them at a local server.

| Pipe | Source | What it proves |
|---|---|---|
| `campus-week` | St. Olaf ICS | TZID times, all-day dates, `GEO`, `CATEGORIES`, `expand rrule`, `during` |
| `carleton-week` | Carleton ICS (2,392 events, 3.2 MB) plus the calendar's HTML page (`--page`) | Floating times plus `X-WR-TIMEZONE`; HTML in `DESCRIPTION` (renamed to `description_html`); HTML-escaped URLs; the same event under several UIDs (`uniq-by summary start`); pictures, which only the page has, joined on the event code (`/_c/<code>` at the end of the description, `?eId=<code>` on the page) with `query web`. If the page fails, the events come without pictures and a `pipe warn` |
| `northfield` | Northfield ICS (`catID=41`) | VTIMEZONE with nested RRULEs; `DTSTAMP` with a TZID; HTML and a trailing ` -` in `LOCATION`; a `URL` that points back at the feed (the event page is taken from the description instead) |
| `stolaf-wp` | Tribe API | Paging through `next_rest_url`, failing past `--max-pages` rather than returning part of the calendar; date filters sent in the calendar's zone; venues, cost, organizers |
| `presence` | Presence API | UTC instants with no `tz`; organizer as org plus contact person; cover photos (`--photos`) |
| `all-campus` | `pipe get` on the others | Composition; deduplication on `summary` + `start`; sorted by start. A source that fails is left out with a `pipe warn` (stderr under `ppp run`), and the rest still come through |

## 8. Errors and limits

### 8.1 Errors

CLI exit codes: **0** success, **1** validation failed, **2** pipe or usage error.

| Kind | Behavior |
|---|---|
| Parse or runtime error in a pipe | Nushell's own diagnostic (file, line, span) on stderr |
| HTTP status other than 2xx | Error naming the URL and status |
| Network failure with a cached copy | `http get` returns the stale copy and warns on stderr |
| Validation failure | For each record: pipe, index, JSON Pointer, message, short excerpt of the value. The CLI prints the first 20, then the total |
| Blocked command | Not registered, so nushell reports "command not found" |
| Cycle in `pipe get` | Error showing the chain |
| Runaway recurrence | `expand rrule` fails at 10,000 occurrences in a series |

### 8.2 Limits

- **Time:** `--timeout` (default 60 s), enforced through nushell's interrupt signal.
- **Response size:** `http get` refuses bodies over 20 MB by default.
- **Memory:** there is no in-process memory limit. One cannot be enforced well inside a single process, so the response-size cap stands in for now. The server work will revisit this, for example by running pipes in a worker process.

---

## 9. Testing

Unit and integration tests run on saved copies of real feeds. Nothing is mocked. Tests follow red/green TDD.

- **`from ics`**: the KDL examples converted to `.ics` (`campus-events` and `team-planning` include RRULEs); saved copies of all five live feeds; and hand-written cases for folded lines, TEXT escapes, quoted parameters, nested VTIMEZONE RRULEs, a floating time with no zone (must fail), `DURATION` without `DTEND`, and a malformed `DTSTAMP`.
- **`expand rrule`**: RRULE with EXDATE and RDATE; RECURRENCE-ID overrides, including one moved into the window; a weekly event across both DST changes; a time in the spring-forward gap and in the fall-back hour; floating and all-day masters; the 10,000 cap.
- **`serialize`**: each of the three time formats; the `when` rule; a time-field map built from every stdlib type; the walker's error on an unsupported keyword.
- **Schemas**: every `fixtures/examples/*.json` passes. Every `fixtures/invalid/*.json` fails at its expected JSON Pointer. These are written fresh, one mistake per file, because `invalid/broken.kdl` from the KDL project is missing.
- **`ppp` module**: `from tribe-events`, `from presence` and `into event` run in the embedded engine on saved API responses.
- **Both wrappers**: each Rust command runs through its in-process `Command` and through `nu-plugin-test-support`, with the same inputs and expected outputs.
- **Sandbox**: `^ls`, `run-external`, `save`, `cd` and `rm` are not registered.
- **Names**: none of our commands shadows a nushell built-in.
- **CLI**: `ppp run`, `info` and `validate` against pipes in `fixtures/` whose input data is written inline (the sandbox has no file access); checks exit codes and stderr.
- **Live acceptance** (`#[ignore]`, run with `cargo test -- --ignored`): the six acceptance pipes fetch their real URLs, and every record must validate.

---

## 10. Build order

1. Workspace; `common` and `calendar` schemas; fixtures; schema registry, validator and `serialize`; `ppp validate`.
2. Engine; `from ics`; `ppp` module with `into event`; `campus-week`; `ppp run`.
3. `expand rrule`.
4. HTTP cache; `ppp info`; `pipe get`; sandbox and limits.
5. `html to-text`; `from tribe-events`; `from presence`; the other acceptance pipes; `all-campus`.
6. `nu_plugin_pipes`.

---

## 11. Changes from the brainstorm

- §6.2 says `into <type>` formats datetimes. Here `serialize` does it at the boundary, from the schema (§5.3), and `into event` only reshapes.
- §6.4 lists memory limits. This prototype defers them (§8.2).
- §6.5 says the host keeps the last good output. This is a server feature and is out of scope.
- §7 says to "convert `invalid/broken.kdl`". That file is missing, so the invalid fixtures are written fresh.
- `pipes/campus-week.nu` called `from ics | where start < …` on the official `from ics`, which has no `start` column. Our `from ics` provides it.
- The `event` record gains `rdates`, `series` and `recurrence_id`.
