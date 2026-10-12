# Upcoming events from the City of Northfield's community calendar.

export const pipe = {
    output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"]
    refresh: 6hr
}

# Northfield community events
#
# Upcoming events from the City of Northfield's calendar.
@category pipes
@search-terms calendar events northfield city
@example "Next month" { northfield --within 4wk }
export def main [
    --within: duration = 4wk  # How far ahead to look
    --from: datetime          # Start of the window (default: now)
    --source: string = "https://www.northfieldmn.gov/common/modules/iCalendar/iCalendar.aspx?catID=41&feed=calendar"  # The iCalendar feed
]: nothing -> table {
    use pipes *
    let start = $from | default (date now)
    let until = $start + $within
    http get --raw $source
    | from ics
    # Each event's URL points back at the feed; its page is in the description.
    # Locations are HTML with a trailing " -".
    | update url {|row|
        let page = $row.description? | default "" | parse -r '(?<url>https?://\S+)' | get url.0?
        $page | default ($row.url | if $in != null { url parse --base $source | url join })
    }
    | update location {|row|
        if $row.location != null { $row.location | html to-text | str replace -r '\s*-\s*$' '' }
    }
    | expand rrule --from $start --until $until
    | during $start $until
    | into event --source $source
}
