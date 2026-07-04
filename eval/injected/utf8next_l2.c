// Derived from jq (src/jv_unicode.c)
// at 71c2ab509a8628dbbad4bc7b3f98a64aa90d3297, MIT licensed.
// Planted-clone eval asset for semdup; not production code.
// Same behavior as jq's jvp_utf8_next, restructured: each case returns
// immediately instead of falling through to a shared epilogue, and the
// validity checks collapse into one condition.

#include <assert.h>
#include "jv_utf8_tables.h"

const char* utf8_advance(const char* cur, const char* limit, int* out) {
  assert(cur <= limit);
  if (cur == limit) return 0;

  unsigned char lead = (unsigned char)cur[0];
  int width = utf8_coding_length[lead];

  if (lead < 0x80) {
    /* ASCII */
    *out = lead;
    return cur + 1;
  }
  if (width == 0 || width == UTF8_CONTINUATION_BYTE) {
    /* invalid lead byte or stray continuation byte */
    *out = -1;
    return cur + 1;
  }
  if (cur + width > limit) {
    /* truncated sequence at end of input */
    *out = -1;
    return limit;
  }

  int value = lead & utf8_coding_bits[lead];
  for (int i = 1; i < width; i++) {
    unsigned char trail = (unsigned char)cur[i];
    if (utf8_coding_length[trail] != UTF8_CONTINUATION_BYTE) {
      /* sequence cut short by a non-continuation byte */
      *out = -1;
      return cur + i;
    }
    value = (value << 6) | (trail & 0x3f);
  }

  int overlong = value < utf8_first_codepoint[width];
  int surrogate = 0xD800 <= value && value <= 0xDFFF;
  int out_of_range = value > 0x10FFFF;
  *out = (overlong || surrogate || out_of_range) ? -1 : value;
  return cur + width;
}
