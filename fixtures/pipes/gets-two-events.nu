export const pipe = { output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"] }

# A pipe that gets another pipe's records.
export def main []: nothing -> table {
    pipe get two-events {count: 1} | update summary {|row| $"Got: ($row.summary)" }
}
