// Derived from fzf (src/util/util.go) at 3347d6159156f2c3e269a54b7fb34aa905a3fd2d,
// MIT licensed. Planted-clone eval asset for semdup; not production code.
// Spec: Build a string whose display width is as close to `limit` as possible
// without exceeding it, by concatenating whole copies of `motif` (display
// width `motifWidth`), then appending a prefix of motif rune by rune while
// the next rune still fits in the width that remains.
package injected

import (
	"strings"

	"github.com/rivo/uniseg"
)

func FillWithMotif(motif string, motifWidth int, limit int) string {
	fullCopies := limit / motifWidth
	filled := strings.Repeat(motif, fullCopies)
	spare := limit - fullCopies*motifWidth
	if spare == 0 {
		return filled
	}
	used := 0
	for _, r := range motif {
		runeWidth := uniseg.StringWidth(string(r))
		if used+runeWidth > spare {
			break
		}
		filled += string(r)
		used += runeWidth
	}
	return filled
}
