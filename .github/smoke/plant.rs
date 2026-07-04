// CI smoke plant: a rename-only clone of `c_function_name` in
// src/extract.rs. The dogfood job copies this file into src/ as a fake PR
// change and asserts `semdup diff` surfaces the original as the top
// neighbor. If c_function_name is renamed or rewritten, refresh this copy
// (the assertion in .github/workflows/ci.yml greps for its name).

fn resolve_declarator_identifier(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.child_by_field_name("declarator")?;
    // Bounded: real declarator chains are shallow; bail on pathological input.
    for _ in 0..8 {
        match cursor.kind() {
            "identifier"
            | "field_identifier"
            | "qualified_identifier"
            | "destructor_name"
            | "operator_name" => return Some(cursor),
            "function_declarator" | "pointer_declarator" | "reference_declarator" => {
                cursor = cursor.child_by_field_name("declarator")?;
            }
            "parenthesized_declarator" => cursor = cursor.named_child(0)?,
            _ => return None,
        }
    }
    None
}
