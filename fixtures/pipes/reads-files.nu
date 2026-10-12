export const pipe = { output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"] }

# A pipe that tries to read the filesystem, which the sandbox forbids.
export def main []: nothing -> table {
    ls / | each { {type: event, id: $in.name, summary: $in.name, start: 2026-10-14T19:30:00Z} }
}
