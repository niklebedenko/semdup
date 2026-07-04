// Derived from gson (gson/src/main/java/com/google/gson/stream/JsonWriter.java)
// at 29e3d1d2cc0ce4175378e511a87f538561625515, Apache-2.0 licensed.
// Planted-clone eval asset for semdup; not production code.

import java.io.IOException;
import java.io.Writer;

class QuotedEmitter {
  private Writer sink;
  private boolean htmlMode;
  private static final String[] BASIC_TABLE = new String[128];
  private static final String[] HTML_SAFE_TABLE = new String[128];

  private void emitQuoted(String text) throws IOException {
    String[] table = htmlMode ? HTML_SAFE_TABLE : BASIC_TABLE;
    sink.write('\"');
    int flushedUpTo = 0;
    int size = text.length();
    for (int pos = 0; pos < size; pos++) {
      char ch = text.charAt(pos);
      String escaped;
      if (ch < 128) {
        escaped = table[ch];
        if (escaped == null) {
          continue;
        }
      } else if (ch == '\u2028') {
        escaped = "\\u2028";
      } else if (ch == '\u2029') {
        escaped = "\\u2029";
      } else {
        continue;
      }
      if (flushedUpTo < pos) {
        sink.write(text, flushedUpTo, pos - flushedUpTo);
      }
      sink.write(escaped);
      flushedUpTo = pos + 1;
    }
    if (flushedUpTo < size) {
      sink.write(text, flushedUpTo, size - flushedUpTo);
    }
    sink.write('\"');
  }
}
