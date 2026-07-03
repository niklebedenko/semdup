// Derived from vuejs/core (packages/shared/src/normalizeProp.ts) at 6eb29d345aa73746207f80c89ee8b37ff7b949c9,
// MIT license. Planted-clone eval asset for semdup; not production code.

// Turns a nested class-attribute spec into one space-separated string.
export function buildSpaceDelimited(spec: any): string {
  if (typeof spec === 'string') {
    return spec.trim()
  }
  if (Array.isArray(spec)) {
    return spec
      .map((entry: any) => buildSpaceDelimited(entry))
      .filter((part: string) => part.length > 0)
      .join(' ')
  }
  if (spec !== null && typeof spec === 'object') {
    return Object.keys(spec)
      .filter(key => !!spec[key])
      .join(' ')
      .trim()
  }
  return ''
}
