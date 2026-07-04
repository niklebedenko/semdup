// Derived from fzf (src/util/util.go) at 3347d6159156f2c3e269a54b7fb34aa905a3fd2d,
// MIT licensed. Planted-clone eval asset for semdup; not production code.
package injected

import (
	"strconv"
	"strings"
)

// OrderVersionStrings compares two dotted version strings.
func OrderVersionStrings(left, right string) int {
	leftFields := strings.Split(left, ".")
	rightFields := strings.Split(right, ".")

	toNumber := func(field string) int {
		value, err := strconv.Atoi(field)
		if err != nil {
			return 0
		}
		return value
	}

	for idx := 0; idx < biggest(len(leftFields), len(rightFields)); idx++ {
		var a, b int
		if idx < len(leftFields) {
			a = toNumber(leftFields[idx])
		}
		if idx < len(rightFields) {
			b = toNumber(rightFields[idx])
		}

		if a > b {
			return 1
		} else if a < b {
			return -1
		}
	}
	return 0
}

func biggest(first int, second int) int {
	if first > second {
		return first
	}
	return second
}
