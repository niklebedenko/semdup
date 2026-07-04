// Derived from Newtonsoft.Json (Src/Newtonsoft.Json/Utilities/StringUtils.cs)
// at 0a2e291c0d9c0c7675d445703e51750363a549ef, MIT licensed.
// Planted-clone eval asset for semdup; not production code.
// Same behavior as Newtonsoft's ToSeparatedCase, restructured: three boolean
// flags track the word state instead of an enum state machine.

using System.Text;

class DelimiterCase
{
    public static string Rewrite(string source, char mark)
    {
        if (string.IsNullOrEmpty(source))
        {
            return source;
        }

        var target = new StringBuilder();
        bool inWord = false;
        bool upperRun = false;
        bool breakPending = false;

        for (int i = 0; i < source.Length; i++)
        {
            char c = source[i];
            if (c == ' ')
            {
                // Spaces are dropped; remember one word break at most.
                if (inWord)
                {
                    breakPending = true;
                }
                continue;
            }
            if (c == mark)
            {
                target.Append(mark);
                inWord = false;
                upperRun = false;
                breakPending = false;
                continue;
            }
            if (char.IsUpper(c))
            {
                if (breakPending || (inWord && !upperRun))
                {
                    target.Append(mark);
                }
                else if (upperRun && i > 0 && i + 1 < source.Length)
                {
                    // An uppercase run's last letter starts the next word.
                    char next = source[i + 1];
                    if (!char.IsUpper(next) && next != mark)
                    {
                        target.Append(mark);
                    }
                }
                target.Append(char.ToLowerInvariant(c));
                inWord = true;
                upperRun = true;
                breakPending = false;
                continue;
            }
            if (breakPending)
            {
                target.Append(mark);
            }
            target.Append(c);
            inWord = true;
            upperRun = false;
            breakPending = false;
        }

        return target.ToString();
    }
}
