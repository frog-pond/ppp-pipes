# KDL calendar event schema

- `calendar.schema.kdl`: KDL Schema 1.0.0 for an event calendar (KDL v2 syntax).
- `examples/`: documents that validate against it. `stolaf-feed.kdl` is three real events from the St. Olaf WordPress feed.
- `invalid/broken.kdl`: a document that should fail, one mistake per line.
- `validate.mjs`: a small validator covering the schema features used here.

Fields mirror iCalendar VEVENT and the columns from `nushell-ics/ics.nu`:

| ics.nu column | KDL |
|---|---|
| uid | `event "<uid>"` |
| summary, description, url, status, rrule | same-named child nodes |
| start, end | `start` / `end` (`tz=` optional, else the calendar's `tz=`) |
| all_day | `start ... all-day=#true` |
| location, geo | `location "..." { geo lat=... lon=... }` |
| categories | `category "A" "B"` |
| organizer | `organizer "Name" email=...` |

Extras beyond ics.nu: `duration`, `attendee`, `exdate`, `reminder`.

Fields from The Events Calendar's WordPress feed (`tribe/events/v1`, added in 0.2.0):

| feed field | KDL |
|---|---|
| website | `link "..." rel=tickets\|registration\|info\|livestream\|other` |
| cost, cost_details | `cost free`, `cost 90 currency=USD`, `cost 5 10 currency=USD` |
| tags | `keyword "A" "B"` |
| excerpt | `excerpt "..."` (plain text) |
| date_utc, modified_utc | `created`, `modified` |
| venue | calendar-level `venue <id> { name address locality region postcode country phone url geo }`, used via `location venue=<id>` |
| organizer phone, website | `organizer ... phone=... url=...` (multiple organizers allowed) |
| image | `image "..." alt="..."` |
| is_virtual, virtual_url | `attendance in-person\|online\|hybrid`, `online "..."` |
| featured, hide_from_listings | `event ... featured=#true hidden=#true` |

KDL Schema can't check that `location venue=` names a venue that exists; an app has to.

Converting that feed: strip HTML from description/excerpt, decode entities (`&amp;`, `&#8211;`), add `https://` to bare websites, and turn all-day `23:59:59` ends into exclusive next-day ends.

```sh
npm i kdljs@0.3.0
node validate.mjs calendar.schema.kdl examples/*.kdl   # all pass
node validate.mjs calendar.schema.kdl invalid/broken.kdl # 14 errors
```
