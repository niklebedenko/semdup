// Derived from vuejs/core (packages/shared/src/looseEqual.ts) at 6eb29d345aa73746207f80c89ee8b37ff7b949c9,
// MIT license. Planted-clone eval asset for semdup; not production code.

// Tolerant deep comparison; primitives of differing types fall back to
// string coercion, so 1 and "1" are considered equivalent.
export function equivalentLoose(left: any, right: any): boolean {
  if (left === right) return true

  const leftDate = left instanceof Date
  const rightDate = right instanceof Date
  if (leftDate || rightDate) {
    return leftDate && rightDate && left.getTime() === right.getTime()
  }

  if (typeof left === 'symbol' || typeof right === 'symbol') {
    return left === right
  }

  const leftArr = Array.isArray(left)
  const rightArr = Array.isArray(right)
  if (leftArr || rightArr) {
    if (!(leftArr && rightArr) || left.length !== right.length) {
      return false
    }
    return left.every((item: any, idx: number) =>
      equivalentLoose(item, right[idx]),
    )
  }

  const leftObj = left !== null && typeof left === 'object'
  const rightObj = right !== null && typeof right === 'object'
  if (leftObj || rightObj) {
    if (!leftObj || !rightObj) {
      return false
    }
    if (Object.keys(left).length !== Object.keys(right).length) {
      return false
    }
    for (const prop in left) {
      const ownL = Object.prototype.hasOwnProperty.call(left, prop)
      const ownR = Object.prototype.hasOwnProperty.call(right, prop)
      if (ownL !== ownR || !equivalentLoose(left[prop], right[prop])) {
        return false
      }
    }
  }

  return String(left) === String(right)
}
