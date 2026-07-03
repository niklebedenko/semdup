// Derived from ripgrep (crates/core/flags/parse.rs) at 4649aa9700619f94cf9c66876e9549d83420e16c,
// dual-licensed MIT OR Unlicense. Planted-clone eval asset for semdup; not production code.

// Spec: Given an unrecognized flag name typed by the user, scan the global
// registry of known flags and gather every name -- the primary long name,
// the negated form when one exists, and each alias -- whose trigram-set
// Jaccard similarity to the typed name meets a fixed 0.4 threshold. The
// survivors are returned in registry order (primary, then negated, then
// aliases per flag) for use in a "did you mean" style hint.

fn spelling_candidates(typed: &str) -> Vec<&'static str> {
    let typed_grams = trigram_bag(typed);
    let mut out: Vec<&'static str> = Vec::new();
    for entry in REGISTRY.iter() {
        let mut forms: Vec<&'static str> = Vec::with_capacity(4);
        forms.push(entry.name_long());
        if let Some(negated) = entry.name_negated() {
            forms.push(negated);
        }
        for alias in entry.aliases() {
            forms.push(alias);
        }
        let mut i = 0;
        while i < forms.len() {
            let grams = trigram_bag(forms[i]);
            if overlap_score(&typed_grams, &grams) >= 0.4 {
                out.push(forms[i]);
            }
            i += 1;
        }
    }
    out
}
