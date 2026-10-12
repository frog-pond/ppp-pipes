# Upcoming events from St. Olaf's WordPress calendar (The Events Calendar).
# It has venues, costs and organizers that the iCalendar feed lacks.

export const pipe = {
    output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"]
    refresh: 1hr
}

# St. Olaf events (WordPress)
#
# Upcoming events from St. Olaf's WordPress calendar, with venues and costs.
@category pipes
@search-terms calendar events
@example "Next three days" { stolaf-wp --within 3day }
export def main [
    --within: duration = 1wk  # How far ahead to look
    --from: datetime          # Start of the window (default: now)
    --max-pages: int = 20     # Safety cap on requests
    --source: string = "https://wp.stolaf.edu/calendar/wp-json/tribe/events/v1/events"  # The API's events URL
]: nothing -> table {
    use pipes *
    let start = $from | default (date now)
    let until = $start + $within
    # The API reads its date filters in the calendar's own zone and compares
    # whole days, so ask for a day more on each side; `during` trims the rest.
    # `start_date`/`end_date` would miss events already under way.
    let local = {|at| $at | date to-timezone America/Chicago | format date "%Y-%m-%d %H:%M:%S" }
    let first = $source
        | url parse
        | merge {params: {
            per_page: 50
            ends_after: (do $local ($start - 1day))
            starts_before: (do $local ($until + 1day))
        }}
        | url join
    # Follow `next_rest_url`, but never past --max-pages: a partial calendar
    # that looks complete is worse than a failed run.
    let pages = generate {|state|
        let page = http get $state.url
        let next = $page.next_rest_url?
        if ($next | is-empty) {
            {out: $page}
        } else if $state.count >= $max_pages {
            error make {msg: $"more than ($max_pages) pages; raise --max-pages"}
        } else {
            {out: $page, next: {url: $next, count: ($state.count + 1)}}
        }
    } {url: $first, count: 1}
    | collect
    $pages
    | each { $in.events? | default [] }
    | flatten
    | from tribe-events
    | during $start $until
    | into event --source $source
}
