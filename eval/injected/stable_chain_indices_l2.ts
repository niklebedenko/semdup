// Derived from vuejs/core (packages/runtime-core/src/renderer.ts) at 6eb29d345aa73746207f80c89ee8b37ff7b949c9,
// MIT license. Planted-clone eval asset for semdup; not production code.

function lowerBound(seq: number[], tails: number[], target: number): number {
  let low = 0
  let high = tails.length - 1
  while (low < high) {
    const middle = (low + high) >> 1
    if (seq[tails[middle]] < target) {
      low = middle + 1
    } else {
      high = middle
    }
  }
  return low
}

// Indices of a longest strictly increasing subsequence (zeros ignored).
export function risingIndexPath(seq: number[]): number[] {
  const parent = new Array<number>(seq.length)
  const tails: number[] = [0]
  seq.forEach((val, pos) => {
    if (val === 0) return
    const lastIdx = tails[tails.length - 1]
    if (seq[lastIdx] < val) {
      parent[pos] = lastIdx
      tails.push(pos)
      return
    }
    const slot = lowerBound(seq, tails, val)
    if (val < seq[tails[slot]]) {
      if (slot > 0) {
        parent[pos] = tails[slot - 1]
      }
      tails[slot] = pos
    }
  })
  let trail = tails[tails.length - 1]
  for (let k = tails.length - 1; k >= 0; k--) {
    tails[k] = trail
    trail = parent[trail]
  }
  return tails
}
