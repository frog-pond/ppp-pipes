export const pipe = { output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"] }

# The other half: gets cycle-a.
export def main []: nothing -> table { pipe get cycle-a }
