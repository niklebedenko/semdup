// Derived from jq (src/jv_unicode.c)
// at 71c2ab509a8628dbbad4bc7b3f98a64aa90d3297, MIT licensed.
// Planted-clone eval asset for semdup; not production code.
// Spec: Encode a Unicode scalar value (0..0x10FFFF) as UTF-8 into `out`,
// which must have room for four bytes, and return the number of bytes
// written: 1 for values up to 0x7F, 2 up to 0x7FF, 3 up to 0xFFFF, 4 above.
// Multi-byte sequences use the standard leading-byte prefixes
// (0xC0/0xE0/0xF0) with 6-bit continuation bytes tagged 0x80.

#include <assert.h>

int utf8_emit(unsigned int scalar, char* out) {
  assert(scalar <= 0x10FFFF);
  if (scalar < 0x80) {
    out[0] = (char)scalar;
    return 1;
  }
  if (scalar < 0x800) {
    out[0] = (char)(0xC0 | (scalar >> 6));
    out[1] = (char)(0x80 | (scalar & 0x3F));
    return 2;
  }
  if (scalar < 0x10000) {
    out[0] = (char)(0xE0 | (scalar >> 12));
    out[1] = (char)(0x80 | ((scalar >> 6) & 0x3F));
    out[2] = (char)(0x80 | (scalar & 0x3F));
    return 3;
  }
  out[0] = (char)(0xF0 | (scalar >> 18));
  out[1] = (char)(0x80 | ((scalar >> 12) & 0x3F));
  out[2] = (char)(0x80 | ((scalar >> 6) & 0x3F));
  out[3] = (char)(0x80 | (scalar & 0x3F));
  return 4;
}
