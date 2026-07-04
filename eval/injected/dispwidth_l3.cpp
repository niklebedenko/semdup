// Derived from fmt (include/fmt/format.h)
// at 123913715afeb8a437e6388b4473fcc4753e1c9a, MIT licensed.
// Planted-clone eval asset for semdup; not production code.
// Spec: Estimate how many terminal columns a UTF-8 string occupies. Iterate
// its codepoints; each counts one column, except members of the wide East
// Asian / pictographic blocks (Hangul Jamo initial consonants, the CJK
// angle brackets, CJK Unified through Yi minus the ideographic half-fill
// space, Hangul syllables, CJK compatibility ideographs and forms, vertical
// and fullwidth forms, supplementary CJK planes, and the emoji blocks
// U+1F300-1F64F and U+1F900-1F9FF), which count two.

#include <cstddef>
#include <cstdint>
#include <string_view>

template <typename F> void for_each_codepoint(std::string_view s, F f);

inline size_t column_width(std::string_view utf8) {
  auto is_double_width = [](uint32_t c) -> bool {
    return (c >= 0x1100 && c <= 0x115f) || c == 0x2329 || c == 0x232a ||
           (c >= 0x2e80 && c <= 0xa4cf && c != 0x303f) ||
           (c >= 0xac00 && c <= 0xd7a3) || (c >= 0xf900 && c <= 0xfaff) ||
           (c >= 0xfe10 && c <= 0xfe19) || (c >= 0xfe30 && c <= 0xfe6f) ||
           (c >= 0xff00 && c <= 0xff60) || (c >= 0xffe0 && c <= 0xffe6) ||
           (c >= 0x20000 && c <= 0x2fffd) || (c >= 0x30000 && c <= 0x3fffd) ||
           (c >= 0x1f300 && c <= 0x1f64f) || (c >= 0x1f900 && c <= 0x1f9ff);
  };
  size_t total = 0;
  for_each_codepoint(utf8, [&](uint32_t cp, std::string_view) {
    total += is_double_width(cp) ? 2 : 1;
    return true;
  });
  return total;
}
