// Derived from jq (src/jv_unicode.c)
// at 71c2ab509a8628dbbad4bc7b3f98a64aa90d3297, MIT licensed.
// Planted-clone eval asset for semdup; not production code.
// Spec: Decode the next UTF-8 codepoint from [p, end). Returns NULL when the
// input is empty, otherwise a pointer just past the bytes consumed, storing
// the decoded codepoint (or -1 on malformed input) through *result. ASCII
// consumes one byte. An invalid or stray continuation lead byte consumes one
// byte and yields -1. A sequence truncated by the end of input consumes what
// remains and yields -1; one truncated by a non-continuation byte consumes
// the valid prefix. Overlong encodings, surrogates (U+D800..U+DFFF), and
// values above U+10FFFF decode but yield -1. No lookup tables are used.

static int lead_width(unsigned char b) {
  if (b < 0x80) return 1;
  if (b < 0xC0) return -1; /* continuation byte */
  if (b < 0xE0) return 2;
  if (b < 0xF0) return 3;
  if (b < 0xF8) return 4;
  return 0; /* 0xF8..0xFF never appear in UTF-8 */
}

const char* utf8_read(const char* p, const char* end, int* result) {
  if (p >= end) return 0;

  unsigned char lead = (unsigned char)*p;
  int width = lead_width(lead);
  if (width == 1) {
    *result = lead;
    return p + 1;
  }
  if (width <= 0) {
    *result = -1;
    return p + 1;
  }
  if (end - p < width) {
    *result = -1;
    return end;
  }

  int cp = lead & (0x7F >> width);
  for (int i = 1; i < width; i++) {
    unsigned char b = (unsigned char)p[i];
    if ((b & 0xC0) != 0x80) {
      *result = -1;
      return p + i;
    }
    cp = (cp << 6) | (b & 0x3F);
  }

  static const int floor_for[] = {0, 0, 0x80, 0x800, 0x10000};
  if (cp < floor_for[width] || (cp >= 0xD800 && cp <= 0xDFFF) || cp > 0x10FFFF)
    cp = -1;

  *result = cp;
  return p + width;
}
