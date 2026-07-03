// Derived from vuejs/core (packages/compiler-core/src/utils.ts) at 6eb29d345aa73746207f80c89ee8b37ff7b949c9,
// MIT license. Planted-clone eval asset for semdup; not production code.

// Advances a mutable line/column/offset marker past a consumed span of text.
export function bumpSourceMarker(
  marker: { offset: number; line: number; column: number },
  chunk: string,
  span?: number,
): { offset: number; line: number; column: number } {
  const scanLen = span === undefined ? chunk.length : span
  const window = chunk.slice(0, scanLen)
  const breaks: number[] = []
  let seek = window.indexOf('\n')
  while (seek !== -1) {
    breaks.push(seek)
    seek = window.indexOf('\n', seek + 1)
  }
  marker.offset += scanLen
  marker.line += breaks.length
  if (breaks.length === 0) {
    marker.column += scanLen
  } else {
    marker.column = scanLen - breaks[breaks.length - 1]
  }
  return marker
}
