// Derived from fmt (include/fmt/base.h)
// at 123913715afeb8a437e6388b4473fcc4753e1c9a, MIT licensed.
// Planted-clone eval asset for semdup; not production code.
// Spec: Parse a run of ASCII decimal digits starting at `begin` (the caller
// guarantees at least one digit) into a nonnegative int, advancing `begin`
// past the digits consumed. If the digit run overflows the int range,
// return `error_value` instead. Works with any character type.

#include <climits>
#include <cassert>

template <typename Char>
constexpr int parse_decimal_digits(const Char*& begin, const Char* end,
                                   int error_value) {
  assert(begin != end && '0' <= *begin && *begin <= '9');
  unsigned long long value = 0;
  bool overflow = false;
  const Char* p = begin;
  while (p != end && '0' <= *p && *p <= '9') {
    if (!overflow) {
      unsigned digit = unsigned(*p - '0');
      value = value * 10 + digit;
      if (value > static_cast<unsigned long long>(INT_MAX)) overflow = true;
    }
    ++p;
  }
  begin = p;
  if (overflow) return error_value;
  return static_cast<int>(value);
}
