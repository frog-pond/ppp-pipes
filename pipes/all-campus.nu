# Everything happening around campus: St. Olaf (both calendars and student
# orgs), Carleton and Northfield, in one list.

export const pipe = {
    output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"]
    refresh: 1hr
}

# All campus events
#
# Events from every campus pipe, without duplicates, earliest first.
@category pipes
@search-terms calendar events
@example "Next three days" { all-campus --within 3day }
export def main [
    --within: duration = 1wk  # How far ahead to look
    --from: datetime          # Start of the window (default: now)
]: nothing -> table {
    let args = {within: $within} | merge (if $from != null { {from: $from} } else { {} })
    [campus-week stolaf-wp presence carleton-week northfield]
    | each {|name| pipe get $name $args }
    | flatten
    # St. Olaf's two calendars list many of the same events.
    | uniq-by summary start
    | sort-by start
}
