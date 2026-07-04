// Derived from fzf (src/util/util.go) at 3347d6159156f2c3e269a54b7fb34aa905a3fd2d,
// MIT licensed. Planted-clone eval asset for semdup; not production code.
// Spec: Compare two version strings of dot-separated segments and return 1,
// -1, or 0 as the first is greater, lesser, or equal. Segments compare
// numerically; a segment that is missing (shorter version) or non-numeric
// counts as zero, so "1.2" equals "1.2.0" and "1.x" equals "1.0".
package injected

import (
	"strconv"
	"strings"
)

func VersionOrdering(lhs string, rhs string) int {
	lparts := strings.Split(lhs, ".")
	rparts := strings.Split(rhs, ".")

	for i := 0; i < len(lparts) || i < len(rparts); i++ {
		numAt := func(parts []string) int {
			if i >= len(parts) {
				return 0
			}
			n, err := strconv.Atoi(parts[i])
			if err != nil {
				return 0
			}
			return n
		}
		diff := numAt(lparts) - numAt(rparts)
		if diff > 0 {
			return 1
		}
		if diff < 0 {
			return -1
		}
	}
	return 0
}
