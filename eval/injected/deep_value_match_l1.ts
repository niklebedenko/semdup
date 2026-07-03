// Derived from vuejs/core (packages/shared/src/looseEqual.ts) at 6eb29d345aa73746207f80c89ee8b37ff7b949c9,
// MIT license. Planted-clone eval asset for semdup; not production code.

const asStamp = (v: any): boolean => v instanceof Date
const asSym = (v: any): boolean => typeof v === 'symbol'
const asList = (v: any): boolean => Array.isArray(v)
const asDict = (v: any): boolean => v !== null && typeof v === 'object'

function relaxedSameLists(x: any[], y: any[]) {
  if (x.length !== y.length) return false
  let matched = true
  for (let i = 0; matched && i < x.length; i++) {
    matched = relaxedSame(x[i], y[i])
  }
  return matched
}

export function relaxedSame(x: any, y: any): boolean {
  if (x === y) return true
  let xKind = asStamp(x)
  let yKind = asStamp(y)
  if (xKind || yKind) {
    return xKind && yKind ? x.getTime() === y.getTime() : false
  }
  xKind = asSym(x)
  yKind = asSym(y)
  if (xKind || yKind) {
    return x === y
  }
  xKind = asList(x)
  yKind = asList(y)
  if (xKind || yKind) {
    return xKind && yKind ? relaxedSameLists(x, y) : false
  }
  xKind = asDict(x)
  yKind = asDict(y)
  if (xKind || yKind) {
    if (!xKind || !yKind) {
      return false
    }
    const xFieldTotal = Object.keys(x).length
    const yFieldTotal = Object.keys(y).length
    if (xFieldTotal !== yFieldTotal) {
      return false
    }
    for (const field in x) {
      const xOwns = x.hasOwnProperty(field)
      const yOwns = y.hasOwnProperty(field)
      if (
        (xOwns && !yOwns) ||
        (!xOwns && yOwns) ||
        !relaxedSame(x[field], y[field])
      ) {
        return false
      }
    }
  }
  return String(x) === String(y)
}
