// Derived from vuejs/core (packages/compiler-core/src/utils.ts) at 6eb29d345aa73746207f80c89ee8b37ff7b949c9,
// MIT license. Planted-clone eval asset for semdup; not production code.

type Caret = {
  offset: number
  line: number
  column: number
}

export function shiftCaretInPlace(
  caret: Caret,
  text: string,
  charCount: number = text.length,
): Caret {
  let breakTally = 0
  let finalBreakAt = -1
  for (let idx = 0; idx < charCount; idx++) {
    if (text.charCodeAt(idx) === 10 /* "\n" */) {
      breakTally++
      finalBreakAt = idx
    }
  }

  caret.offset += charCount
  caret.line += breakTally
  caret.column =
    finalBreakAt === -1 ? caret.column + charCount : charCount - finalBreakAt

  return caret
}
