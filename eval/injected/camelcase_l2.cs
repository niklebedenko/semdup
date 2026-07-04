// Derived from Newtonsoft.Json (Src/Newtonsoft.Json/Utilities/StringUtils.cs)
// at 0a2e291c0d9c0c7675d445703e51750363a549ef, MIT licensed.
// Planted-clone eval asset for semdup; not production code.
// Same behavior as Newtonsoft's ToCamelCase, restructured: measure the
// leading uppercase run first, then lowercase a computed prefix.

class CamelStyler
{
    public static string DecapitalizePrefix(string text)
    {
        if (string.IsNullOrEmpty(text) || !char.IsUpper(text[0]))
        {
            return text;
        }

        int run = 1;
        while (run < text.Length && char.IsUpper(text[run]))
        {
            run++;
        }

        int lowered;
        if (run == text.Length)
        {
            lowered = run;
        }
        else if (run == 1)
        {
            lowered = 1;
        }
        else if (char.IsSeparator(text[run]))
        {
            // "FOO bar" -> "foo bar": the whole run drops when a separator
            // follows it.
            lowered = run;
        }
        else
        {
            // The run's last letter starts the next word and keeps its case.
            lowered = run - 1;
        }

        var chars = text.ToCharArray();
        for (int i = 0; i < lowered; i++)
        {
            chars[i] = char.ToLowerInvariant(chars[i]);
        }
        return new string(chars);
    }
}
