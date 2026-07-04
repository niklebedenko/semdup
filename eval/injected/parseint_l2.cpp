// Derived from fmt (include/fmt/base.h)
// at 123913715afeb8a437e6388b4473fcc4753e1c9a, MIT licensed.
// Planted-clone eval asset for semdup; not production code.
// Same behavior as fmt's parse_nonnegative_int, restructured: overflow is
// detected inside the scan loop with a flag instead of by re-checking the
// digit count and last digit afterwards.

#include <climits>
#include <cassert>

template <typename Char>
constexpr auto read_arg_index(const Char*& pos, const Char* end,
                              int on_overflow) noexcept -> int {
  assert(pos != end && '0' <= *pos && *pos <= '9');
  unsigned long long acc = 0;
  bool overflowed = false;
  const Char* it = pos;
  while (it != end && '0' <= *it && *it <= '9') {
    if (!overflowed) {
      acc = acc * 10 + static_cast<unsigned long long>(*it - '0');
      if (acc > static_cast<unsigned long long>(INT_MAX)) overflowed = true;
    }
    ++it;
  }
  pos = it;
  return overflowed ? on_overflow : static_cast<int>(acc);
}
