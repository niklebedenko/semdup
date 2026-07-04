// Derived from fmt (include/fmt/format.h)
// at 123913715afeb8a437e6388b4473fcc4753e1c9a, MIT licensed.
// Planted-clone eval asset for semdup; not production code.
// Same behavior as fmt's compute_width, restructured: the wide-codepoint
// test walks a table of ranges instead of one boolean expression, and the
// per-codepoint visitor is a lambda.

#include <cstddef>
#include <cstdint>
#include <string_view>

template <typename F> void for_each_codepoint(std::string_view s, F f);

// Approximate display width of a UTF-8 string: wide East-Asian and
// pictographic codepoints count as two columns, everything else as one.
inline auto display_columns(std::string_view text) -> size_t {
  struct span {
    uint32_t lo, hi;
  };
  static constexpr span wide[] = {
      {0x1100, 0x115f},    // Hangul Jamo initial consonants
      {0x2329, 0x232a},    // angle brackets
      {0x2e80, 0xa4cf},    // CJK ... Yi (0x303f carved out below)
      {0xac00, 0xd7a3},    // Hangul Syllables
      {0xf900, 0xfaff},    // CJK Compatibility Ideographs
      {0xfe10, 0xfe19},    // Vertical Forms
      {0xfe30, 0xfe6f},    // CJK Compatibility Forms
      {0xff00, 0xff60},    // Fullwidth Forms
      {0xffe0, 0xffe6},    // Fullwidth Forms
      {0x20000, 0x2fffd},  // CJK
      {0x30000, 0x3fffd},
      {0x1f300, 0x1f64f},  // Misc Symbols and Pictographs + Emoticons
      {0x1f900, 0x1f9ff},  // Supplemental Symbols and Pictographs
  };
  size_t columns = 0;
  for_each_codepoint(text, [&](uint32_t cp, std::string_view) -> bool {
    size_t w = 1;
    if (cp != 0x303f) {  // IDEOGRAPHIC HALF FILL SPACE stays narrow
      for (const span& r : wide) {
        if (cp >= r.lo && cp <= r.hi) {
          w = 2;
          break;
        }
      }
    }
    columns += w;
    return true;
  });
  return columns;
}
