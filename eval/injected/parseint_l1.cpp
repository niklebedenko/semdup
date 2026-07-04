// Derived from fmt (include/fmt/base.h)
// at 123913715afeb8a437e6388b4473fcc4753e1c9a, MIT licensed.
// Planted-clone eval asset for semdup; not production code.

#include <climits>
#include <cassert>

template <typename Char>
constexpr auto scan_decimal_index(const Char*& begin, const Char* end,
                                  int error_value) noexcept -> int {
  assert(begin != end && '0' <= *begin && *begin <= '9');
  unsigned value = 0, prev = 0;
  auto p = begin;
  do {
    prev = value;
    value = value * 10 + unsigned(*p - '0');
    ++p;
  } while (p != end && '0' <= *p && *p <= '9');
  auto num_digits = p - begin;
  begin = p;
  int digits10 = static_cast<int>(sizeof(int) * CHAR_BIT * 3 / 10);
  if (num_digits <= digits10) return static_cast<int>(value);
  // Check for overflow.
  unsigned max = INT_MAX;
  return num_digits == digits10 + 1 &&
                 prev * 10ull + unsigned(p[-1] - '0') <= max
             ? static_cast<int>(value)
             : error_value;
}
