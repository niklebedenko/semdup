// Derived from gson (gson/src/main/java/com/google/gson/FieldNamingPolicy.java)
// at 29e3d1d2cc0ce4175378e511a87f538561625515, Apache-2.0 licensed.
// Planted-clone eval asset for semdup; not production code.
// Same behavior as gson's upperCaseFirstLetter, restructured: mutate a char
// buffer in place instead of splicing substrings.

class TitleCaseHelper {
  static String promoteFirstAlphabetic(String input) {
    char[] buffer = input.toCharArray();
    for (int i = 0; i < buffer.length; i++) {
      if (!Character.isLetter(buffer[i])) {
        continue;
      }
      if (Character.isUpperCase(buffer[i])) {
        return input;
      }
      buffer[i] = Character.toUpperCase(buffer[i]);
      return new String(buffer);
    }
    return input;
  }
}
