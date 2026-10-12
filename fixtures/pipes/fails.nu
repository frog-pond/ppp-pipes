export const pipe = { output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"] }

# A pipe that fails while running.
export def main []: nothing -> table {
    error make {msg: "the source is down"}
}
