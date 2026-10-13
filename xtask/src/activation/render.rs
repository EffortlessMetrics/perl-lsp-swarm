//! Deterministic reviewer-readable rendering of the activation inventory
//! (#9204). Row order always follows `rows` (already sorted by
//! `surface_id`), grouped by class in [`ActivationClass::all`] order.

use super::model::{ActivationClass, ActivationInventory};

/// Render one line per row, grouped by class, plus a summary header.
#[must_use]
pub fn render_list(inventory: &ActivationInventory) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "activation inventory ({}, {}): {} row(s)\n",
        inventory.schema_version,
        inventory.controlling_issue,
        inventory.rows.len()
    ));
    for class in ActivationClass::all() {
        let rows: Vec<_> = inventory.rows.iter().filter(|row| row.class == *class).collect();
        out.push_str(&format!("\n{} ({})\n", class.as_str(), rows.len()));
        for row in rows {
            out.push_str(&format!(
                "  {}  owner={}  authority={}\n",
                row.surface_id, row.owner, row.class_authority.authority
            ));
        }
    }
    out.push_str("\nderivation\n");
    for entry in &inventory.derivation {
        out.push_str(&format!(
            "  {}: authority={} considered={} emitted={}\n",
            entry.rule, entry.authority, entry.considered, entry.emitted
        ));
    }
    out
}
