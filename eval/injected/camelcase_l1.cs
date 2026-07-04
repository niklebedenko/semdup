// Derived from Newtonsoft.Json (Src/Newtonsoft.Json/Utilities/StringUtils.cs)
// at 0a2e291c0d9c0c7675d445703e51750363a549ef, MIT licensed.
// Planted-clone eval asset for semdup; not production code.

class NameBending
{
    public static string LowerLeadingRun(string value)
    {
        if (string.IsNullOrEmpty(value) || !char.IsUpper(value[0]))
        {
            return value;
        }

        char[] letters = value.ToCharArray();

        for (int idx = 0; idx < letters.Length; idx++)
        {
            if (idx == 1 && !char.IsUpper(letters[idx]))
            {
                break;
            }

            bool more = (idx + 1 < letters.Length);
            if (idx > 0 && more && !char.IsUpper(letters[idx + 1]))
            {
                // The leading uppercase run ends at an upper letter followed
                // by a lower one, but a following separator (a space is not
                // uppercase, which is how we got here) still lowercases the
                // current letter, so 'FOO bar' becomes 'foo bar' rather than
                // 'foO bar'.
                if (char.IsSeparator(letters[idx + 1]))
                {
                    letters[idx] = char.ToLowerInvariant(letters[idx]);
                }

                break;
            }

            letters[idx] = char.ToLowerInvariant(letters[idx]);
        }

        return new string(letters);
    }
}
