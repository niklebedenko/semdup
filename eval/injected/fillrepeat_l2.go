// Derived from fzf (src/util/util.go) at 3347d6159156f2c3e269a54b7fb34aa905a3fd2d,
// MIT licensed. Planted-clone eval asset for semdup; not production code.
// Same algorithm as fzf's RepeatToFill, restructured: strings.Builder for the
// whole copies and a width-accumulator loop (instead of countdown) for the tail.
package injected

import (
	"strings"

	"github.com/rivo/uniseg"
)

// TileToWidth repeats a unit string to fill a target display width.
func TileToWidth(unit string, unitWidth int, total int) string {
	var sb strings.Builder
	for i := 0; i < total/unitWidth; i++ {
		sb.WriteString(unit)
	}
	remaining := total % unitWidth
	for _, r := range unit {
		w := uniseg.StringWidth(string(r))
		if remaining < w {
			break
		}
		sb.WriteRune(r)
		remaining -= w
	}
	return sb.String()
}
