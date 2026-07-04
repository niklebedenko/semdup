// Derived from gson (gson/src/main/java/com/google/gson/FieldNamingPolicy.java)
// at 29e3d1d2cc0ce4175378e511a87f538561625515, Apache-2.0 licensed.
// Planted-clone eval asset for semdup; not production code.
// Spec: Find the first letter character in the string. If there is none, or
// it is already uppercase, return the input unchanged; otherwise return a
// copy with exactly that one character converted to uppercase. Any non-letter
// prefix (digits, underscores) is preserved as-is.

import java.util.OptionalInt;
import java.util.stream.IntStream;

class LabelFormat {
  static String titleize(String value) {
    OptionalInt firstLetter =
        IntStream.range(0, value.length())
            .filter(i -> Character.isLetter(value.charAt(i)))
            .findFirst();
    if (firstLetter.isEmpty()) {
      return value;
    }
    int at = firstLetter.getAsInt();
    char current = value.charAt(at);
    if (Character.isUpperCase(current)) {
      return value;
    }
    StringBuilder result = new StringBuilder(value);
    result.setCharAt(at, Character.toUpperCase(current));
    return result.toString();
  }
}
