// Derived from Newtonsoft.Json (Src/Newtonsoft.Json/Utilities/StringUtils.cs)
// at 0a2e291c0d9c0c7675d445703e51750363a549ef, MIT licensed.
// Planted-clone eval asset for semdup; not production code.
// Spec: If the string is empty or does not start with an uppercase letter,
// return it unchanged. Otherwise lowercase the leading run of uppercase
// letters, except that when the run is followed by a lowercase letter its
// final letter keeps its case as the start of the next word (a single-letter
// run never protects itself, and a run followed by a separator character is
// lowercased in full). Lowercasing is culture-invariant.

class Camelizer
{
    public static string Convert(string name)
    {
        if (string.IsNullOrEmpty(name) || !char.IsUpper(name[0]))
        {
            return name;
        }

        int run = 1;
        while (run < name.Length && char.IsUpper(name[run]))
        {
            run++;
        }

        bool protectLast = run < name.Length && run > 1 && !char.IsSeparator(name[run]);
        int stop = protectLast ? run - 1 : run;

        return name.Substring(0, stop).ToLowerInvariant() + name.Substring(stop);
    }
}
