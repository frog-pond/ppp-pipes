export const pipe = { output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"] }

# Half of a loop: gets cycle-b.
export def main []: nothing -> table { pipe get cycle-b }
