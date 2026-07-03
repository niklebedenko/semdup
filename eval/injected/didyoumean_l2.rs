// Derived from ripgrep (crates/core/flags/parse.rs) at 4649aa9700619f94cf9c66876e9549d83420e16c,
// dual-licensed MIT OR Unlicense. Planted-clone eval asset for semdup; not production code.

// Every spelling a single option answers to, in declaration order.
fn spellings_of(flag: &&'static dyn Flag) -> Vec<&'static str> {
    let mut names = vec![flag.name_long()];
    if let Some(neg) = flag.name_negated() {
        names.push(neg);
    }
    names.extend(flag.aliases().iter().copied());
    names
}

// Gather option names whose trigram profile sits near the typo.
fn near_misses(entered: &str) -> Vec<&'static str> {
    const MIN_SCORE: f64 = 0.4;
    let target = trigram_bag(entered);
    REGISTRY
        .iter()
        .flat_map(spellings_of)
        .filter(|name| {
            let candidate = trigram_bag(name);
            overlap_score(&target, &candidate) >= MIN_SCORE
        })
        .collect()
}
