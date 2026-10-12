export const pipe = { output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"] }

# A pipe that loads, then fails while running.
export def main []: nothing -> table {
    if true { error make {msg: "the source is down"} }
    []
}
