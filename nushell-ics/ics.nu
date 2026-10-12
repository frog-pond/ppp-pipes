# ics.nu: parse iCalendar (RFC 5545) text into a table of events.
#
#   use ics.nu *
#   open --raw feed.ics | from ics
#   open feed.ics                    # works too: `open` picks up `from ics` by extension
#   stolaf events | where start > (date now) | first 10
#
# Handles CRLF/LF line endings, line unfolding, quoted property parameters,
# TEXT unescaping, comma-separated list values, TZID/UTC/floating date-times,
# all-day dates, and DURATION in place of DTEND. RRULEs are returned as-is,
# not expanded.

const STOLAF_URL = "https://www.stolaf.edu/apps/calendar/ical.cfm"

# Undo TEXT escaping (\n \N \, \; \\).
def unescape-text []: string -> string {
  str replace -a '\\' (char nul)
  | str replace -ar '\\[nN]' (char nl)
  | str replace -a '\,' ','
  | str replace -a '\;' ';'
  | str replace -a (char nul) '\'
}

# Split a TEXT list on unescaped commas, unescaping each item.
def split-text-list []: string -> list<string> {
  str replace -a '\\' (char nul)
  | str replace -a '\,' (char us)
  | split row ','
  | each { str replace -a (char us) '\,' | str replace -a (char nul) '\\' | unescape-text | str trim }
  | compact --empty
}

# Unfold continuation lines and split into content lines.
def content-lines []: string -> list<string> {
  str replace -a "\r\n" "\n"
  | str replace -ar "\n[ \t]" ''
  | lines
  | where $it != ''
}

# "NAME;P1=a;P2=\"b:c\":value" -> {name, params, value}
def parse-property []: string -> record {
  let m = $in | parse -r '^(?<name>[A-Za-z0-9-]+)(?<params>(?:;[A-Za-z0-9-]+=(?:"[^"]*"|[^";:]*)(?:,(?:"[^"]*"|[^";:]*))*)*):(?<value>.*)$'
  if ($m | is-empty) { return null }
  let m = $m | first
  let params = $m.params
    | parse -r ';(?<k>[A-Za-z0-9-]+)=(?<v>(?:"[^"]*"|[^";:]*)(?:,(?:"[^"]*"|[^";:]*))*)'
    | update k { str uppercase }
    | update v { str trim -c '"' }
    | transpose -r -d
  {name: ($m.name | str uppercase), params: ($params | default {}), value: $m.value}
}

# Interpret a wall-clock time in an IANA zone. Two passes so the offset is
# the one in effect at that wall time, not at the same instant in UTC.
def wall-time-in [tz: string]: string -> datetime {
  let wall = $in
  let off_at = {|dt| $dt | date to-timezone $tz | into record | get timezone }
  let guess = $"($wall)+00:00" | into datetime
  let guess = $"($wall)(do $off_at $guess)" | into datetime
  $"($wall)(do $off_at $guess)" | into datetime | date to-timezone $tz
}

# DTSTART/DTEND-style property -> {at: datetime, all_day: bool}
def ics-datetime [default_tz: string]: record -> record {
  let p = $in
  let v = $p.value | str trim
  if ($p.params.VALUE? == 'DATE') or ($v =~ '^\d{8}$') {
    let wall = $v | parse -r '(?<y>\d{4})(?<m>\d\d)(?<d>\d\d)' | first | $"($in.y)-($in.m)-($in.d)T00:00:00"
    return {at: ($wall | wall-time-in $default_tz), all_day: true}
  }
  let c = $v | parse -r '^(?<y>\d{4})(?<mo>\d\d)(?<d>\d\d)T(?<h>\d\d)(?<mi>\d\d)(?<s>\d\d)(?<z>Z?)$'
  if ($c | is-empty) { error make {msg: $"unrecognized date-time: ($v)"} }
  let c = $c | first
  let wall = $"($c.y)-($c.mo)-($c.d)T($c.h):($c.mi):($c.s)"
  let at = if $c.z == 'Z' {
    $"($wall)+00:00" | into datetime
  } else {
    let tz = $p.params.TZID? | default $default_tz
    try { $wall | wall-time-in $tz } catch { $wall | wall-time-in $default_tz }
  }
  {at: $at, all_day: false}
}

# ISO 8601 / RFC 5545 duration ("P1DT2H30M", "-PT15M", "P2W") -> duration
def ics-duration []: string -> duration {
  let m = $in | parse -r '^(?<sign>[+-]?)P(?:(?<w>\d+)W)?(?:(?<d>\d+)D)?(?:T(?:(?<h>\d+)H)?(?:(?<mi>\d+)M)?(?:(?<s>\d+)S)?)?$' | first
  let n = {|x| if ($x | is-empty) { 0 } else { $x | into int } }
  let total = (do $n $m.w) * 1wk + (do $n $m.d) * 1day + (do $n $m.h) * 1hr + (do $n $m.mi) * 1min + (do $n $m.s) * 1sec
  if $m.sign? == '-' { -1 * $total } else { $total }
}

# Parse iCalendar text into a table of VEVENTs.
#
# Columns: summary, start, end, all_day, location, description, url,
# categories, organizer, geo, uid, status, rrule, created, last_modified,
# and `props`, the full list of {name, params, value} for anything else.
export def "from ics" [
  --tz: string  # zone for floating times and all-day dates (default: the feed's first VTIMEZONE, else local)
]: string -> table {
  let text = $in
  let all = $text | content-lines

  let feed_tz = $all
    | where $it starts-with 'TZID:' or $it starts-with 'X-WR-TIMEZONE:'
    | first 1 | each { split row ':' | skip 1 | str join ':' } | get 0?
  let default_tz = $tz | default $feed_tz | default 'local'
  let default_tz = try { date now | date to-timezone $default_tz | ignore; $default_tz } catch { 'local' }

  $text
  | str replace -a "\r\n" "\n"
  | split row "BEGIN:VEVENT\n" | skip 1
  | each { split row "END:VEVENT" | first }
  # drop nested components (VALARM etc.)
  | each { str replace -ar '(?s)BEGIN:[A-Z-]+\n.*?END:[A-Z-]+\n?' '' }
  | each {|block|
      let props = $block | content-lines | each { parse-property } | compact
      let one = {|name| $props | where name == $name | get 0? }
      let text_of = {|name| do $one $name | if $in == null { null } else { $in.value | unescape-text } }
      let dt_of = {|name| do $one $name | if $in == null { null } else { $in | ics-datetime $default_tz } }

      let start = do $dt_of DTSTART
      let end = do $dt_of DTEND | default (
        do $one DURATION | if $in == null or $start == null { null } else {
          {at: ($start.at + ($in.value | ics-duration)), all_day: $start.all_day}
        }
      )
      let organizer = do $one ORGANIZER | if $in == null { null } else {
        {name: $in.params.CN?, email: ($in.value | str replace -r '(?i)^mailto:' '')}
      }
      let geo = do $one GEO | if $in == null { null } else {
        $in.value | split row ';' | into float | {lat: $in.0, lon: $in.1}
      }

      {
        summary: (do $text_of SUMMARY)
        start: $start.at?
        end: $end.at?
        all_day: ($start.all_day? | default false)
        location: (do $text_of LOCATION)
        description: (do $text_of DESCRIPTION)
        url: (do $one URL | get value?)
        categories: ($props | where name == CATEGORIES | each { $in.value | split-text-list } | flatten)
        organizer: $organizer
        geo: $geo
        uid: (do $one UID | get value?)
        status: (do $one STATUS | get value?)
        rrule: (do $one RRULE | get value?)
        created: (do $dt_of CREATED | get at?)
        last_modified: (do $dt_of LAST-MODIFIED | get at?)
        props: $props
      }
    }
}

# Fetch St. Olaf's public events calendar. The endpoint needs no query
# parameters; `--category` passes its `category` filter through (e.g. music).
export def "stolaf events" [
  --category: string  # category slug to filter by
]: nothing -> table {
  let url = if $category == null { $STOLAF_URL } else {
    {category: $category} | url build-query | $"($STOLAF_URL)?($in)"
  }
  http get --raw $url | from ics
}
