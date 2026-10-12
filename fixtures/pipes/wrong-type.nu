export const pipe = { output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"] }

# A pipe that returns a record type it does not declare.
export def main []: nothing -> table { [{type: entry, id: "x"}] }
