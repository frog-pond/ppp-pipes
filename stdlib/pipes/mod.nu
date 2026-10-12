# The pipes standard library: helpers that turn source data into stdlib records.

# iCalendar properties that `into event` maps to event fields. Every other
# property is kept under `extra`, keyed by its lower-cased name.
const MAPPED_PROPS = [
    UID SUMMARY DESCRIPTION LOCATION URL STATUS DTSTART DTEND DURATION CATEGORIES
    ORGANIZER GEO RRULE RDATE EXDATE RECURRENCE-ID CREATED LAST-MODIFIED
]

# Remove the fields whose value is null, "", [] or {}.
export def drop-empty []: record -> record {
    items {|key, value|
        if $value in [null "" [] {}] { null } else { {key: $key, value: $value} }
    }
    | compact
    | reduce --fold {} {|field, acc| $acc | insert $field.key $field.value }
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

# Turn rows from `from ics` into stdlib `event` records.
#
# An ORGANIZER becomes a person when its CUTYPE is INDIVIDUAL and an
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
    let has_location = $row.location? | is-not-empty
    let has_web_url = ($row.url? | default "") =~ '^https?://'
    let leftovers = $row.props? | default [] | where name not-in $MAPPED_PROPS | group-by name
        | items {|name, props|
            let values = $props | get value
            {key: ($name | str lowercase), value: (if ($values | length) == 1 { $values.0 } else { $values })}
        }
        | reduce --fold {} {|field, acc| $acc | insert $field.key $field.value }
    let extra = $leftovers
        | merge (if $has_web_url or $row.url? == null { {} } else { {url: $row.url} })
        | merge (if $has_location or $row.geo? == null { {} } else { {geo: $row.geo} })

    {type: event}
    | merge $identity
    | merge {
        source: $source
        summary: $row.summary?
        start: $row.start?
        end: $row.end?
        all_day: ($row.all_day? | default false)
        tz: ($row.tz? | default $tz)
        status: (if $status in [tentative confirmed cancelled] { $status })
        location: (if $has_location { {name: $row.location} | merge (if $row.geo? != null { {geo: $row.geo} } else { {} }) })
        description: $row.description?
        url: (if $has_web_url { $row.url })
        categories: ($row.categories? | default [] | uniq)
        organizers: ($row.organizer? | organizers)
        rrule: $row.rrule?
        rdates: ($row.rdates? | default [])
        exdates: ($row.exdates? | default [])
        recurrence_id: $row.recurrence_id?
        created: $row.created?
        modified: $row.last_modified?
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
