# A test pipe that needs no network: it reads two events from inline text.

export const pipe = {
    output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"]
}

# Two events
#
# Both from an inline calendar.
export def main [
    --count: int = 2          # How many events to return
    --prefix: string = ""     # Text put before each summary
    --loud                    # Shout the summaries
]: nothing -> table {
    use ppp *
    "BEGIN:VCALENDAR
X-WR-TIMEZONE:America/Chicago
BEGIN:VEVENT
UID:a@example.edu
SUMMARY:Chess Club
DTSTART:20261013T180000
END:VEVENT
BEGIN:VEVENT
UID:b@example.edu
SUMMARY:Choir
DTSTART:20261014T190000
END:VEVENT
END:VCALENDAR"
    | from ics
    | first $count
    | update summary {|row| $"($prefix)($row.summary)" | if $loud { str uppercase } else { $in } }
    | into event
}
