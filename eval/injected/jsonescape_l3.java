// Derived from gson (gson/src/main/java/com/google/gson/stream/JsonWriter.java)
// at 29e3d1d2cc0ce4175378e511a87f538561625515, Apache-2.0 licensed.
// Planted-clone eval asset for semdup; not production code.
// Spec: Emit `payload` to `target` as a double-quoted JSON string literal.
// Characters below 128 with a non-null entry in the `escapes` table are
// replaced by that entry; U+2028 and U+2029 are always written as unicode
// escapes; everything else passes through unchanged. Contiguous unescaped
// stretches must be written in bulk rather than one character at a time.

import java.io.IOException;
import java.io.Writer;

class LiteralWriter {
  static void writeLiteral(Writer target, String payload, String[] escapes) throws IOException {
    target.write('"');
    int start = 0;
    while (start < payload.length()) {
      int stop = start;
      String pending = null;
      while (stop < payload.length()) {
        char candidate = payload.charAt(stop);
        if (candidate < 128 && escapes[candidate] != null) {
          pending = escapes[candidate];
          break;
        }
        if (candidate == '\u2028' || candidate == '\u2029') {
          pending = String.format("\\u%04x", (int) candidate);
          break;
        }
        stop++;
      }
      target.write(payload, start, stop - start);
      if (pending != null) {
        target.write(pending);
        stop++;
      }
      start = stop;
    }
    target.write('"');
  }
}
