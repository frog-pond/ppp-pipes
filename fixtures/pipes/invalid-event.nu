export const pipe = { output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"] }

# A pipe that returns an event with no start.
export def main []: nothing -> table { [{type: event, id: "x", summary: "Lost"}] }
