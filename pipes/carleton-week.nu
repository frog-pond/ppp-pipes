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
    --page: string = "https://www.carleton.edu/calendar/"  # The calendar's web page, which has the events' pictures
]: nothing -> table {
    use ppp *
    let start = $from | default (date now)
    let until = $start + $within
    let images = event-images $page $start $until
    http get --raw $source
    | from ics
    # Carleton puts HTML in DESCRIPTION, a TEXT field, and HTML-escapes its URLs.
    | rename --column {description: description_html}
    | update url {|row| $row.url | html to-text }
    | expand rrule --from $start --until $until
    | during $start $until
    # The feed merges several calendars, so one event can appear more than once.
    | uniq-by summary start
    | insert images {|row|
        let code = $row.description_html? | default "" | str trim | parse -r '/_c/(?<code>[A-Za-z0-9]+)$' | get code.0?
        let url = if $code != null { $images | get --optional $code }
        if $url != null { [{url: $url, alt: ($row.summary | html to-text)}] } else { [] }
    }
    | into event --source $source
}

# The feed has no pictures; the calendar's page does. Each event there links
# its picture to `?eId=<code>`, and the feed's description ends with the same
# code in a `/_c/<code>` link. Returns a record of code -> full-size image
# URL. The pictures are a nicety: if the page fails, the events come without.
def event-images [page: string, start: datetime, until: datetime]: nothing -> record {
    let day = {|at| $at | date to-timezone America/Chicago | format date "%Y-%m-%d" }
    try {
        # Keep any query --page already has, but with our dates.
        let page_url = $page | url parse
        let params = $page_url.params
            | where key not-in [start_date end_date]
            | append [{key: start_date, value: (do $day $start)} {key: end_date, value: (do $day $until)}]
        let url = $page_url | reject query | merge {params: $params} | url join
        http get --raw $url
        | query html --query '.event_list--item picture a[href*="eId="]' --as-html
        | each {|link|
            let code = $link | query html --query a --attribute href | get 0? | default ""
                | parse -r 'eId=(?<code>[A-Za-z0-9]+)' | get code.0?
            # The page shows a 100x100 crop; without the query the CDN serves the original.
            let image = $link | query html --query img --attribute src | get 0?
                | if $in != null { split row '?' | first | url parse --base $url | reject params }
                # Inline (`data:`) placeholders are not pictures an app can link to.
                | if $in != null and $in.scheme in [http https] { url join }
            if $code != null and $image != null { {code: $code, image: $image} }
        }
        | compact
        | uniq-by code
        | each {|found| [$found.code $found.image] }
        | into record
    } catch {|err|
        pipe warn $"Carleton event pictures are missing: ($err.msg)"
        {}
    }
}
