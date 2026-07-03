// Derived from vuejs/core (packages/compiler-core/src/utils.ts) at 6eb29d345aa73746207f80c89ee8b37ff7b949c9,
// MIT license. Planted-clone eval asset for semdup; not production code.

// Spec: Mutates a source-position record (offset, line, column) to account
// for consuming the first N characters of a text chunk, where N defaults to
// the whole chunk. The offset grows by N and the line grows by the number of
// newline characters among those N. If at least one newline was consumed,
// the column restarts as N minus the index of the last consumed newline;
// otherwise the column simply advances by N. Returns the same record.

export function consumeIntoPosition(
  state: { offset: number; line: number; column: number },
  chunk: string,
  taken: number = chunk.length,
): { offset: number; line: number; column: number } {
  const eaten = chunk.slice(0, taken)
  const newlines = eaten.split('\n').length - 1
  state.offset += taken
  state.line += newlines
  if (newlines > 0) {
    const lastBreak = eaten.lastIndexOf('\n')
    state.column = taken - lastBreak
  } else {
    state.column += taken
  }
  return state
}
