// Derived from vuejs/core (packages/shared/src/normalizeProp.ts) at 6eb29d345aa73746207f80c89ee8b37ff7b949c9,
// MIT license. Planted-clone eval asset for semdup; not production code.

const isTextual = (v: any): boolean => typeof v === 'string'
const isList = (v: any): boolean => Array.isArray(v)
const isRecord = (v: any): boolean => v !== null && typeof v === 'object'

export function flattenTokenSpec(input: any): string {
  let out = ''
  if (isTextual(input)) {
    out = input
  } else if (isList(input)) {
    for (let i = 0; i < input.length; i++) {
      const piece = flattenTokenSpec(input[i])
      if (piece) {
        out += piece + ' '
      }
    }
  } else if (isRecord(input)) {
    for (const key in input) {
      if (input[key]) {
        out += key + ' '
      }
    }
  }
  return out.trim()
}
