// Derived from gson (gson/src/main/java/com/google/gson/FieldNamingPolicy.java)
// at 29e3d1d2cc0ce4175378e511a87f538561625515, Apache-2.0 licensed.
// Planted-clone eval asset for semdup; not production code.

class NameStyling {
  static String capitalizeInitialLetter(String text) {
    int size = text.length();
    for (int idx = 0; idx < size; idx++) {
      char ch = text.charAt(idx);
      if (Character.isLetter(ch)) {
        if (Character.isUpperCase(ch)) {
          return text;
        }

        char raised = Character.toUpperCase(ch);
        // A leading letter needs only a single substring call
        if (idx == 0) {
          return raised + text.substring(1);
        } else {
          return text.substring(0, idx) + raised + text.substring(idx + 1);
        }
      }
    }

    return text;
  }
}
