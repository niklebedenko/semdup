// Derived from ripgrep (crates/core/flags/parse.rs) at 4649aa9700619f94cf9c66876e9549d83420e16c,
// dual-licensed MIT OR Unlicense. Planted-clone eval asset for semdup; not production code.

// Collect registered option names resembling the mystery token.
fn related_options(mystery: &str) -> Vec<&'static str> {
    // Minimum overlap score before we bother surfacing a candidate to
    // the person at the terminal.
    const CUTOFF: f64 = 0.4;

    let mut matches = vec![];
    let probe = trigram_bag(mystery);
    for &opt in REGISTRY.iter() {
        let label = opt.name_long();
        let bag = trigram_bag(label);
        if overlap_score(&probe, &bag) >= CUTOFF {
            matches.push(label);
        }
        if let Some(label) = opt.name_negated() {
            let bag = trigram_bag(label);
            if overlap_score(&probe, &bag) >= CUTOFF {
                matches.push(label);
            }
        }
        for label in opt.aliases() {
            let bag = trigram_bag(label);
            if overlap_score(&probe, &bag) >= CUTOFF {
                matches.push(label);
            }
        }
    }
    matches
}
