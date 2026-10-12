# Upcoming events from St. Olaf's student organizations, on Presence.

export const pipe = {
    output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"]
    refresh: 1hr
}

# Student org events
#
# Upcoming events from St. Olaf student organizations (Presence).
@category pipes
@search-terms calendar events student organizations
@example "Next three days" { presence --within 3day }
export def main [
    --within: duration = 1wk  # How far ahead to look
    --from: datetime          # Start of the window (default: now)
    --source: string = "https://api.presence.io/stolaf/v1/events"  # The API's events URL
    --photos: string = "https://stolaf-cdn.presence.io/event-photos/09ddef77-5009-4348-8540-c9bfc6ade6bc/"  # Where cover photos live
]: nothing -> table {
    use pipes *
    let start = $from | default (date now)
    http get $source
    | from presence --photos $photos
    | during $start ($start + $within)
    | into event --source $source
}
