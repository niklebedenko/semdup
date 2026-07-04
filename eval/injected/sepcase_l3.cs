// Derived from Newtonsoft.Json (Src/Newtonsoft.Json/Utilities/StringUtils.cs)
// at 0a2e291c0d9c0c7675d445703e51750363a549ef, MIT licensed.
// Planted-clone eval asset for semdup; not production code.
// Spec: Convert a name to lowercase separator-delimited form (snake/kebab
// style). Uppercase letters are lowercased. A separator is inserted before
// an uppercase letter that follows a lowercase letter, and before the last
// uppercase letter of an uppercase run when a non-uppercase, non-separator
// character follows it. Runs of spaces between words collapse into a single
// separator (leading and trailing spaces vanish). Existing separators pass
// through and reset the word tracking.

using System.Text;

class WordCasing
{
    public static string Separate(string name, char separator)
    {
        if (string.IsNullOrEmpty(name))
        {
            return name;
        }

        StringBuilder sb = new StringBuilder(name.Length);
        bool inWord = false;      // emitted a word character since the last separator
        bool spaceGap = false;    // spaces seen since the last emitted character

        for (int i = 0; i < name.Length; i++)
        {
            char c = name[i];
            if (c == ' ')
            {
                spaceGap |= inWord;
                continue;
            }
            if (c == separator)
            {
                sb.Append(separator);
                inWord = false;
                spaceGap = false;
                continue;
            }

            bool isUpper = char.IsUpper(c);
            bool needSeparator = spaceGap;
            if (!needSeparator && isUpper && inWord)
            {
                if (!char.IsUpper(name[i - 1]))
                {
                    // lower-to-upper word boundary: camelCase -> camel_case
                    needSeparator = true;
                }
                else if (i + 1 < name.Length && !char.IsUpper(name[i + 1]) && name[i + 1] != separator)
                {
                    // end of an uppercase run: HTTPServer -> http_server
                    needSeparator = true;
                }
            }

            if (needSeparator)
            {
                sb.Append(separator);
            }
            sb.Append(isUpper ? char.ToLowerInvariant(c) : c);
            inWord = true;
            spaceGap = false;
        }

        return sb.ToString();
    }
}
