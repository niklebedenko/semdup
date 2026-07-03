// Derived from vuejs/core (packages/runtime-core/src/renderer.ts) at 6eb29d345aa73746207f80c89ee8b37ff7b949c9,
// MIT license. Planted-clone eval asset for semdup; not production code.

// Spec: Given a numeric array, returns the index positions of one longest
// strictly increasing subsequence, where entries whose value is 0 are never
// candidates. Ties are resolved the patience-sorting way: each new value is
// placed at the leftmost pile whose top is not smaller, so smaller tail
// values are preferred. Quirk preserved from the source: the pile list is
// seeded with index 0, so an empty input still yields [0].

export function ascendingRunPositions(values: number[]): number[] {
  const predecessor = new Map<number, number>()
  const pileTops: number[] = [0]
  for (let at = 0; at < values.length; at++) {
    const sample = values[at]
    if (sample === 0) continue
    // find the leftmost pile whose top value is >= sample
    let where = pileTops.length
    let base = 0
    let span = pileTops.length
    while (span > 0) {
      const half = span >> 1
      const probe = base + half
      if (values[pileTops[probe]] < sample) {
        base = probe + 1
        span -= half + 1
      } else {
        where = probe
        span = half
      }
    }
    if (where === pileTops.length) {
      predecessor.set(at, pileTops[where - 1])
      pileTops.push(at)
    } else if (sample < values[pileTops[where]]) {
      if (where > 0) {
        predecessor.set(at, pileTops[where - 1])
      }
      pileTops[where] = at
    }
  }
  const out: number[] = new Array(pileTops.length)
  let cursor = pileTops[pileTops.length - 1]
  for (let slot = out.length - 1; slot >= 0; slot--) {
    out[slot] = cursor
    cursor = predecessor.get(cursor)!
  }
  return out
}
