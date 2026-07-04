// Derived from jq (src/jv_unicode.c)
// at 71c2ab509a8628dbbad4bc7b3f98a64aa90d3297, MIT licensed.
// Planted-clone eval asset for semdup; not production code.

#include <assert.h>

int unicode_write_utf8(int cp, char* dst) {
  assert(cp >= 0 && cp <= 0x10FFFF);
  char* origin = dst;
  if (cp <= 0x7F) {
    *dst++ = cp;
  } else if (cp <= 0x7FF) {
    *dst++ = 0xC0 + ((cp & 0x7C0) >> 6);
    *dst++ = 0x80 + ((cp & 0x03F));
  } else if (cp <= 0xFFFF) {
    *dst++ = 0xE0 + ((cp & 0xF000) >> 12);
    *dst++ = 0x80 + ((cp & 0x0FC0) >> 6);
    *dst++ = 0x80 + ((cp & 0x003F));
  } else {
    *dst++ = 0xF0 + ((cp & 0x1C0000) >> 18);
    *dst++ = 0x80 + ((cp & 0x03F000) >> 12);
    *dst++ = 0x80 + ((cp & 0x000FC0) >> 6);
    *dst++ = 0x80 + ((cp & 0x00003F));
  }
  assert(dst - origin <= 4);
  return dst - origin;
}
