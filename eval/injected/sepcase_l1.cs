// Derived from Newtonsoft.Json (Src/Newtonsoft.Json/Utilities/StringUtils.cs)
// at 0a2e291c0d9c0c7675d445703e51750363a549ef, MIT licensed.
// Planted-clone eval asset for semdup; not production code.

using System.Text;

class WordSplitter
{
    private enum RunState
    {
        Begin,
        Small,
        Big,
        Fresh
    }

    private static string InsertDividers(string text, char divider)
    {
        if (string.IsNullOrEmpty(text))
        {
            return text;
        }

        StringBuilder built = new StringBuilder();
        RunState state = RunState.Begin;

        for (int pos = 0; pos < text.Length; pos++)
        {
            if (text[pos] == ' ')
            {
                if (state != RunState.Begin)
                {
                    state = RunState.Fresh;
                }
            }
            else if (char.IsUpper(text[pos]))
            {
                switch (state)
                {
                    case RunState.Big:
                        bool more = (pos + 1 < text.Length);
                        if (pos > 0 && more)
                        {
                            char following = text[pos + 1];
                            if (!char.IsUpper(following) && following != divider)
                            {
                                built.Append(divider);
                            }
                        }
                        break;
                    case RunState.Small:
                    case RunState.Fresh:
                        built.Append(divider);
                        break;
                }

                built.Append(char.ToLowerInvariant(text[pos]));
                state = RunState.Big;
            }
            else if (text[pos] == divider)
            {
                built.Append(divider);
                state = RunState.Begin;
            }
            else
            {
                if (state == RunState.Fresh)
                {
                    built.Append(divider);
                }

                built.Append(text[pos]);
                state = RunState.Small;
            }
        }

        return built.ToString();
    }
}
