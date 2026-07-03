// Derived from vuejs/core (packages/shared/src/looseEqual.ts) at 6eb29d345aa73746207f80c89ee8b37ff7b949c9,
// MIT license. Planted-clone eval asset for semdup; not production code.

// Spec: Performs a tolerant deep-equality test between two values. Dates
// compare by timestamp, arrays compare element-wise with the same tolerant
// rule, and plain objects must have matching own-key sets whose values are
// tolerantly equal. Any other pair — and any object pair that passes the
// structural scan — falls back to comparing the two values' string
// coercions, so for example 1 and "1" count as equal.

export function fuzzyDeepCheck(first: any, second: any): boolean {
  if (first === second) return true
  const kindOf = (v: any): string => {
    if (v instanceof Date) return 'date'
    if (typeof v === 'symbol') return 'symbol'
    if (Array.isArray(v)) return 'list'
    if (v && typeof v === 'object') return 'map'
    return 'basic'
  }
  const k1 = kindOf(first)
  const k2 = kindOf(second)
  switch (k1 === k2 ? k1 : 'mixed') {
    case 'date':
      return first.getTime() === second.getTime()
    case 'symbol':
      return false
    case 'mixed':
      return false
    case 'list': {
      if (first.length !== second.length) return false
      for (let idx = 0; idx < first.length; idx++) {
        if (!fuzzyDeepCheck(first[idx], second[idx])) return false
      }
      return true
    }
    case 'map': {
      const namesA = Object.keys(first)
      const namesB = Object.keys(second)
      if (namesA.length !== namesB.length) return false
      for (const name of namesA) {
        if (!Object.prototype.hasOwnProperty.call(second, name)) return false
        if (!fuzzyDeepCheck(first[name], second[name])) return false
      }
      return String(first) === String(second)
    }
    default:
      return String(first) === String(second)
  }
}
