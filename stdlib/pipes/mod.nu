# The pipes standard library: helpers that turn source data into stdlib records.

# iCalendar properties that `into event` maps to event fields. Every other
# property is kept under `extra`, keyed by its lower-cased name.
const MAPPED_PROPS = [
    UID SUMMARY DESCRIPTION LOCATION URL STATUS DTSTART DTEND DURATION CATEGORIES
    ORGANIZER GEO RRULE RDATE EXDATE RECURRENCE-ID CREATED LAST-MODIFIED
]

# Remove the fields whose value is null, "", [] or {}.
export def drop-empty []: record -> record {
    items {|key, value| [$key $value] }
    | where {|pair| $pair.1 not-in [null "" [] {}] }
    | into record
}

# Keep the rows whose time span overlaps the window [from, until). A row runs
# from `start` to `end`; a row with no `end` is a moment at `start`. Events
# already under way when the window opens are kept.
export def during [from: datetime, until: datetime]: table -> table {
    where {|row|
        if $row.end? == null {
            $row.start >= $from and $row.start < $until
        } else {
            $row.start < $until and $row.end > $from
        }
    }
}

# Turn rows from a format reader (`from ics`, `from tribe-events`,
# `from presence`) into stdlib `event` records.
#
# Rows share one shape: the `from ics` columns, plus optional
# `description_html`, `excerpt`, `place`, `organizers`, `images`, `links`,
# `cost`, `keywords`, `online`, `attendance`, `featured`, `hidden` and `extra`.
# HTML in `summary`, `location`, `excerpt` and `description_html` becomes
# plain text; `description_html` is also kept as `content`.
#
# An ICS ORGANIZER becomes a person when its CUTYPE is INDIVIDUAL and an
# organization otherwise, since campus calendars are mostly run by offices.
# All-day dates that arrive with no time zone are midnight UTC; moving them
# with `date to-timezone` before this command shifts the date.
export def "into event" [
    --source: string  # URL the rows came from, kept on each record
    --tz: string      # Time zone for rows that have none
]: table -> table {
    each {|row| $row | event-from-row $source $tz }
}

def event-from-row [source: any, tz: any]: record -> record {
    let row = $in
    if ($row.uid? | is-empty) {
        error make {msg: $"event has no uid: ($row.summary? | default '(no summary)')"}
    }
    let identity = if $row.recurrence_id? != null {
        let at = $row.recurrence_id | date to-timezone UTC | format date "%Y%m%dT%H%M%SZ"
        {id: $"($row.uid)_($at)", series: $row.uid}
    } else {
        {id: $row.uid}
    }
    let status = $row.status? | default "" | str lowercase
    let location_name = $row.location? | html to-text
    let has_location = ($location_name | is-not-empty) or ($row.place? | is-not-empty)
    let has_web_url = ($row.url? | default "") =~ '^https?://'
    let leftovers = $row.props? | default [] | where name not-in $MAPPED_PROPS | group-by name
        | items {|name, props|
            let values = $props | get value
            [($name | str lowercase) (if ($values | length) == 1 { $values.0 } else { $values })]
        }
        | into record
    let extra = $leftovers
        | merge ($row.extra? | default {})
        | merge (if $has_web_url or $row.url? == null { {} } else { {url: $row.url} })
        | merge (if $has_location or $row.geo? == null { {} } else { {geo: $row.geo} })
    let location = if $has_location {
        {name: $location_name, place: $row.place?, geo: $row.geo?} | drop-empty
    }
    let content = if ($row.description_html? | is-not-empty) {
        {type: html, value: $row.description_html}
    }

    {type: event}
    | merge $identity
    | merge {
        source: $source
        summary: ($row.summary? | html to-text)
        start: $row.start?
        end: $row.end?
        all_day: ($row.all_day? | default false)
        tz: ($row.tz? | default $tz)
        status: (if $status in [tentative confirmed cancelled] { $status })
        location: $location
        attendance: $row.attendance?
        online: $row.online?
        excerpt: ($row.excerpt? | html to-text)
        description: ($row.description? | default ($row.description_html? | html to-text) | if $in != null { str trim })
        content: $content
        url: (if $has_web_url { $row.url })
        links: ($row.links? | default [])
        images: ($row.images? | default [])
        cost: $row.cost?
        categories: ($row.categories? | default [] | uniq)
        keywords: ($row.keywords? | default [] | uniq)
        organizers: ($row.organizers? | default ($row.organizer? | organizers))
        rrule: $row.rrule?
        rdates: ($row.rdates? | default [])
        exdates: ($row.exdates? | default [])
        recurrence_id: $row.recurrence_id?
        created: $row.created?
        modified: $row.last_modified?
        featured: $row.featured?
        hidden: $row.hidden?
        extra: $extra
    }
    | drop-empty
}

# An ICS organizer record ({name, email, cutype}) as a list of stdlib agents.
def organizers []: any -> list<record> {
    let organizer = $in
    if $organizer == null { return [] }
    let name = $organizer.name? | default $organizer.email?
    if $name == null { return [] }
    let emails = if $organizer.email? != null { [{address: $organizer.email}] } else { [] }
    if $organizer.cutype? == "INDIVIDUAL" {
        [{person: ({name: {display: $name}, emails: $emails} | drop-empty)}]
    } else {
        [{org: ({name: $name, emails: $emails} | drop-empty)}]
    }
}

# Turn events from The Events Calendar's WordPress REST API
# (`/wp-json/tribe/events/v1/events`, the `events` list of each page) into
# the rows `into event` reads.
export def "from tribe-events" []: list<any> -> table {
    each {|event| $event | tribe-row }
}

def tribe-row []: record -> record {
    let event = $in
    let utc = {|text| $"($text)+00:00" | into datetime }
    let start = do $utc $event.utc_start_date
    let end = do $utc $event.utc_end_date
    # All-day events end at 23:59:59 on their last day; events end exclusively.
    let end = if $event.all_day and ($event.end_date | str ends-with "23:59:59") { $end + 1sec } else { $end }
    let venue = $event.venue? | if ($in | describe | str starts-with record) { $in }
    let free = ($event.cost_details?.values? | default [] | any {|v| $v == free }) or ($event.cost? =~ '(?i)^\s*free\s*$')
    let amounts = $event.cost_details?.values? | default [] | where {|v| $v =~ '^[0-9.]+$' } | into float
    let cost = if $free {
        {free: true}
    } else if ($amounts | is-not-empty) {
        {min: ($amounts | math min), max: (if ($amounts | length) > 1 { $amounts | math max }), currency: $event.cost_details.currency_code?} | drop-empty
    }
    let website = $event.website? | default "" | html to-text | str trim | str replace -r '(%20)+$' ''
    let image = $event.image? | if ($in | describe | str starts-with record) { $in }
    {
        uid: ($event.global_id? | default ($event.id | into string))
        summary: $event.title
        start: $start
        end: $end
        all_day: $event.all_day
        tz: $event.timezone?
        description_html: $event.description?
        excerpt: $event.excerpt?
        url: $event.url?
        links: (if $website =~ '^https?://' { [{url: $website, rel: related}] } else { [] })
        place: (if $venue != null { $venue | tribe-place })
        organizers: ($event.organizer? | default [] | each {|o| {org: ($o | tribe-org)} })
        cost: $cost
        categories: ($event.categories? | default [] | get name? | default [] | each { html to-text })
        keywords: ($event.tags? | default [] | get name? | default [] | each { html to-text })
        images: (if $image != null { [({url: $image.url, alt: ($event.title | html to-text), width: $image.width?, height: $image.height?} | drop-empty)] } else { [] })
        online: (if ($event.virtual_url? | default "") =~ '^https?://' { {url: $event.virtual_url} })
        created: (do $utc $event.date_utc)
        last_modified: (do $utc $event.modified_utc)
        featured: $event.featured?
        hidden: $event.hide_from_listings?
        extra: (if $cost == null and ($event.cost? | is-not-empty) { {cost: $event.cost} } else { {} })
    }
}

# A Tribe venue as a stdlib place.
def tribe-place []: record -> record {
    let venue = $in
    let country = match ($venue.country? | default "" | str trim | str lowercase) {
        "united states" | "united states of america" | "usa" | "us" => "US"
        "canada" | "ca" => "CA"
        _ => null
    }
    let lat = $venue.geo_lat? | default "" | into string
    let lon = $venue.geo_lng? | default "" | into string
    {
        id: ($venue.id? | into string)
        name: ($venue.venue | html to-text)
        address: ({
            lines: (if ($venue.address? | is-not-empty) { [$venue.address] } else { [] })
            locality: $venue.city?
            region: ($venue.stateprovince? | default $venue.state?)
            postcode: $venue.zip?
            country: $country
        } | drop-empty)
        geo: (if ($lat =~ '^-?[0-9.]+$') and ($lon =~ '^-?[0-9.]+$') { {lat: ($lat | into float), lon: ($lon | into float)} })
        url: (if ($venue.url? | default "") =~ '^https?://' { $venue.url })
    }
    | drop-empty
}

# A Tribe organizer as a stdlib org.
def tribe-org []: record -> record {
    let organizer = $in
    {
        id: ($organizer.id? | into string)
        name: ($organizer.organizer | html to-text)
        emails: (if ($organizer.email? | is-not-empty) { [{address: ($organizer.email | html to-text)}] } else { [] })
        phones: (if ($organizer.phone? | default "") =~ '^[0-9+()./ x-]{3,}$' { [{number: $organizer.phone}] } else { [] })
        url: (if ($organizer.website? | default "") =~ '^https?://' { $organizer.website })
    }
    | drop-empty
}

# Turn events from Presence's events API (`https://api.presence.io/<campus>/v1/events`)
# into the rows `into event` reads. Presence times are instants, so the rows
# have no `tz`.
export def "from presence" [
    --photos: string  # Base URL that cover photos' file names are appended to
]: list<any> -> table {
    each {|event|
        let organizers = [
            ({org: {name: $event.organizationName}})
            (if ($event.contactName? | is-not-empty) {
                {person: ({name: {display: $event.contactName}, emails: (if ($event.contactEmail? | is-not-empty) { [{address: $event.contactEmail}] } else { [] })} | drop-empty)}
            })
        ] | compact
        {
            uid: $event.eventNoSqlId
            summary: $event.eventName
            start: ($event.startDateTimeUtc | into datetime)
            end: ($event.endDateTimeUtc? | if $in != null { into datetime })
            description_html: $event.description?
            location: $event.location?
            url: $"https://($event.subdomain).presence.io/event/($event.uri)"
            images: (if $event.hasCoverImage? == true and $photos != null {
                [{url: $"($photos)($event.photoUriWithVersion)", alt: $event.eventName}]
            } else { [] })
            organizers: $organizers
            keywords: ($event.tags? | default [])
        }
    }
}
