export const pipe = { output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"] }

@deprecated "use something newer"
def old-helper [] { [] }

# A pipe that calls a deprecated command.
export def main []: nothing -> table {
    old-helper
}
