export const pipe = { output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"] }

# A pipe that returns a single record instead of a list.
export def main []: nothing -> record {
    {type: event, id: "solo", summary: "Solo", start: 2026-10-22T16:00:00Z}
}
