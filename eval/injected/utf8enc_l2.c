// Derived from jq (src/jv_unicode.c)
// at 71c2ab509a8628dbbad4bc7b3f98a64aa90d3297, MIT licensed.
// Planted-clone eval asset for semdup; not production code.
// Same behavior as jq's jvp_utf8_encode, restructured: pick the sequence
// length first, then fill the continuation bytes back-to-front in a shared
// loop.

#include <assert.h>

int codepoint_to_utf8(int value, char* buf) {
  assert(value >= 0 && value <= 0x10FFFF);
  int len = value <= 0x7F ? 1 : value <= 0x7FF ? 2 : value <= 0xFFFF ? 3 : 4;
  static const int lead_bits[] = {0, 0x00, 0xC0, 0xE0, 0xF0};
  for (int i = len - 1; i > 0; i--) {
    buf[i] = 0x80 | (value & 0x3F);
    value >>= 6;
  }
  buf[0] = lead_bits[len] | value;
  return len;
}
