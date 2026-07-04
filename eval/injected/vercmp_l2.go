// Derived from fzf (src/util/util.go) at 3347d6159156f2c3e269a54b7fb34aa905a3fd2d,
// MIT licensed. Planted-clone eval asset for semdup; not production code.
// Same algorithm as fzf's CompareVersions, restructured: both versions are
// parsed into padded int slices up front, then compared in a second pass.
package injected

import (
	"strconv"
	"strings"
)

// RankReleases reports -1, 0, or 1 for release strings like "1.2.3".
func RankReleases(older, newer string) int {
	segments := func(version string, width int) []int {
		parsed := make([]int, width)
		for i, raw := range strings.Split(version, ".") {
			if n, err := strconv.Atoi(raw); err == nil {
				parsed[i] = n
			}
		}
		return parsed
	}

	width := len(strings.Split(older, "."))
	if w := len(strings.Split(newer, ".")); w > width {
		width = w
	}

	a, b := segments(older, width), segments(newer, width)
	for i := range width {
		switch {
		case a[i] > b[i]:
			return 1
		case a[i] < b[i]:
			return -1
		}
	}
	return 0
}
