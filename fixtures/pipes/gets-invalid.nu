export const pipe = { output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"] }

# A pipe that gets a pipe whose records fail the check.
export def main []: nothing -> table { pipe get invalid-event }
