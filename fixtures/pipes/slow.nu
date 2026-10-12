export const pipe = { output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"] }

# A pipe that never finishes.
export def main []: nothing -> table {
    loop { }
    []
}
