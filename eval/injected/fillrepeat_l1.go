// Derived from fzf (src/util/util.go) at 3347d6159156f2c3e269a54b7fb34aa905a3fd2d,
// MIT licensed. Planted-clone eval asset for semdup; not production code.
package injected

import (
	"strings"

	"github.com/rivo/uniseg"
)

// PadByRepetition tiles the pattern until the display budget is used up.
func PadByRepetition(pattern string, patternWidth int, budget int) string {
	whole := budget / patternWidth
	leftover := budget % patternWidth
	canvas := strings.Repeat(pattern, whole)
	if leftover > 0 {
		for _, glyph := range pattern {
			leftover -= uniseg.StringWidth(string(glyph))
			if leftover < 0 {
				break
			}
			canvas += string(glyph)
			if leftover == 0 {
				break
			}
		}
	}
	return canvas
}
