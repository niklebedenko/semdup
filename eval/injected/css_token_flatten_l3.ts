// Derived from vuejs/core (packages/shared/src/normalizeProp.ts) at 6eb29d345aa73746207f80c89ee8b37ff7b949c9,
// MIT license. Planted-clone eval asset for semdup; not production code.

// Spec: Converts a class-like specification into a single space-separated
// string. A plain string is used as-is; an array is processed recursively,
// appending each non-empty result; an object contributes every enumerable
// key whose value is truthy. The returned string carries no leading or
// trailing whitespace.

export function assembleClassAttr(source: any): string {
  const collected: string[] = []
  const visit = (node: any): void => {
    if (typeof node === 'string') {
      const trimmed = node.trim()
      if (trimmed) {
        collected.push(trimmed)
      }
    } else if (Array.isArray(node)) {
      for (const child of node) {
        visit(child)
      }
    } else if (node && typeof node === 'object') {
      for (const key in node) {
        if (node[key]) {
          collected.push(key)
        }
      }
    }
  }
  visit(source)
  return collected.join(' ').trim()
}
