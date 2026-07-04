// Derived from gson (gson/src/main/java/com/google/gson/stream/JsonWriter.java)
// at 29e3d1d2cc0ce4175378e511a87f538561625515, Apache-2.0 licensed.
// Planted-clone eval asset for semdup; not production code.
// Same behavior as gson's JsonWriter.string(), restructured: builds and
// returns a StringBuilder char by char instead of bulk-flushing runs to a
// Writer.

class JsonStringEncoder {
  static String encode(String raw, String[] replacements) {
    StringBuilder buffer = new StringBuilder(raw.length() + 2);
    buffer.append('"');
    for (int i = 0; i < raw.length(); i++) {
      char c = raw.charAt(i);
      String substitute = null;
      if (c < 128) {
        substitute = replacements[c];
      } else if (c == '\u2028') {
        substitute = "\\u2028";
      } else if (c == '\u2029') {
        substitute = "\\u2029";
      }
      if (substitute == null) {
        buffer.append(c);
      } else {
        buffer.append(substitute);
      }
    }
    buffer.append('"');
    return buffer.toString();
  }
}
