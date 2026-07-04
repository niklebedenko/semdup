// Derived from jq (src/jv_unicode.c)
// at 71c2ab509a8628dbbad4bc7b3f98a64aa90d3297, MIT licensed.
// Planted-clone eval asset for semdup; not production code.

#include <assert.h>
#include "jv_utf8_tables.h"

const char* utf8_step(const char* in, const char* end, int* codepoint_ret) {
  assert(in <= end);
  if (in == end) {
    return 0;
  }
  int codepoint = -1;
  unsigned char first = (unsigned char)in[0];
  int length = utf8_coding_length[first];
  if ((first & 0x80) == 0) {
    /* Fast-path for ASCII */
    codepoint = first;
    length = 1;
  } else if (length == 0 || length == UTF8_CONTINUATION_BYTE) {
    /* Bad single byte - either an invalid byte or an out-of-place continuation byte */
    length = 1;
  } else if (in + length > end) {
    /* String ends before UTF8 sequence ends */
    length = end - in;
  } else {
    codepoint = ((unsigned)in[0]) & utf8_coding_bits[first];
    for (int i = 1; i < length; i++) {
      unsigned ch = (unsigned char)in[i];
      if (utf8_coding_length[ch] != UTF8_CONTINUATION_BYTE) {
        /* Invalid UTF8 sequence - not followed by the right number of continuation bytes */
        codepoint = -1;
        length = i;
        break;
      }
      codepoint = (codepoint << 6) | (ch & 0x3f);
    }
    if (codepoint < utf8_first_codepoint[length]) {
      /* Overlong UTF8 sequence */
      codepoint = -1;
    }
    if (0xD800 <= codepoint && codepoint <= 0xDFFF) {
      /* Surrogate codepoints can't be encoded in UTF8 */
      codepoint = -1;
    }
    if (codepoint > 0x10FFFF) {
      /* Outside Unicode range */
      codepoint = -1;
    }
  }
  assert(length > 0);
  *codepoint_ret = codepoint;
  return in + length;
}
