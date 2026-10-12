# A sample pipe. It needs stdlib/ics.nu (with `from ics` and `into event`)
# on the module path, since nushell resolves `use` at parse time.
# The host reads everything below with `use campus-week.nu; scope commands`
# and `$campus_week.pipe`, without running `main`.

# Pipe metadata that has no built-in attribute.
export const pipe = {
    output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"]
    refresh: 1hr
}

# This week on campus
#
# Upcoming events from the St. Olaf calendar.
@category pipes
@search-terms calendar events
@example "Next three days" { campus-week --days 3 }
export def main [
    --days: int = 7                    # How far ahead to look
    --tz: string = "America/Chicago"   # Time zone for floating times
]: nothing -> table {
    use stdlib/ics.nu *
    http get "https://www.stolaf.edu/apps/calendar/ical.cfm"
    | from ics
    | where start < ((date now) + ($days * 1day))
    | into event --tz $tz
}
