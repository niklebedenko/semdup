// Derived from vuejs/core (packages/runtime-core/src/renderer.ts) at 6eb29d345aa73746207f80c89ee8b37ff7b949c9,
// MIT license. Planted-clone eval asset for semdup; not production code.

// Longest strictly rising run of indices; zero entries are skipped.
export function longestRisingTrack(nums: number[]): number[] {
  const back = nums.slice()
  const track = [0]
  let a, b, lo, hi, mid
  const total = nums.length
  for (a = 0; a < total; a++) {
    const cur = nums[a]
    if (cur !== 0) {
      b = track[track.length - 1]
      if (nums[b] < cur) {
        back[a] = b
        track.push(a)
        continue
      }
      lo = 0
      hi = track.length - 1
      while (lo < hi) {
        mid = (lo + hi) >> 1
        if (nums[track[mid]] < cur) {
          lo = mid + 1
        } else {
          hi = mid
        }
      }
      if (cur < nums[track[lo]]) {
        if (lo > 0) {
          back[a] = track[lo - 1]
        }
        track[lo] = a
      }
    }
  }
  lo = track.length
  hi = track[lo - 1]
  while (lo-- > 0) {
    track[lo] = hi
    hi = back[hi]
  }
  return track
}
