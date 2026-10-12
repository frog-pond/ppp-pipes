# Upcoming events from the Carleton College campus calendar.

export const pipe = {
    output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"]
    refresh: 1hr
}

# This week at Carleton
#
# Upcoming events from the Carleton campus calendar.
@category pipes
@search-terms calendar events carleton
@example "Next three days" { carleton-week --within 3day }
export def main [
    --within: duration = 1wk  # How far ahead to look
    --from: datetime          # Start of the window (default: now)
    --source: string = "https://www.carleton.edu/calendar/?loadFeed=calendar&stamp=1714843628"  # The iCalendar feed
]: nothing -> table {
    use pipes *
    let start = $from | default (date now)
    let until = $start + $within
    http get --raw $source
    | from ics
    # Carleton puts HTML in DESCRIPTION, a TEXT field, and HTML-escapes its URLs.
    | rename --column {description: description_html}
    | update url {|row| $row.url | html to-text }
    | expand rrule --from $start --until $until
    | during $start $until
    # The feed merges several calendars, so one event can appear more than once.
    | uniq-by summary start
    | into event --source $source
}
