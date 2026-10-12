export const pipe = {
    output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"]
    refresh: 1hr
}

# Events from a URL
#
# For tests: reads an iCalendar feed from --url.
@category pipes
@search-terms calendar test
@example "A local feed" { fetches --url http://localhost:8080/feed }
export def main [
    --url: string  # The feed to read
]: nothing -> table {
    use ppp *
    http get --raw $url | from ics | into event --source $url
}
