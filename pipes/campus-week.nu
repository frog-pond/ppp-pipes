# Upcoming events from the St. Olaf calendar. Run with `ppp run campus-week`;
# the host reads everything above `main`'s body without running it.

# Pipe metadata that has no built-in attribute.
export const pipe = {
    output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"]
    refresh: 1hr
}

const SOURCE = "https://www.stolaf.edu/apps/calendar/ical.cfm"

# This week on campus
#
# Upcoming events from the St. Olaf calendar.
@category pipes
@search-terms calendar events
@example "Next three days" { campus-week --within 3day }
export def main [
    --within: duration = 1wk  # How far ahead to look
    --from: datetime          # Start of the window (default: now)
]: nothing -> table {
    use ppp *
    let now = $from | default (date now)
    let until = $now + $within
    http get --raw $SOURCE
    | from ics
    | expand rrule --from $now --until $until
    | during $now $until
    | into event --source $SOURCE
}
