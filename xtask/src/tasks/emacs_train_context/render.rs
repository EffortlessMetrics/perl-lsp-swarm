//! Deterministic rendering of one `emacs_node_context.v1` packet.
//!
//! JSON rendering serializes the packet struct with serde (field order fixed
//! by declaration, every collection already canonically ordered by the
//! resolver). Markdown rendering is a bounded human navigation view derived
//! only from packet fields. Both are pure functions of the packet: rendering
//! the same packet twice yields identical bytes.

use color_eyre::eyre::Result;

use super::model::NodeContextPacket;

pub fn render_json(packet: &NodeContextPacket) -> Result<String> {
    Ok(serde_json::to_string_pretty(packet)?)
}

pub fn parse_json(raw: &str) -> Result<NodeContextPacket> {
    Ok(serde_json::from_str(raw)?)
}

pub fn render_markdown(packet: &NodeContextPacket) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# emacs_node_context.v1 — {} ({})\n\n",
        packet.node.node_id, packet.status
    ));
    out.push_str(&format!("- issue: #{}\n", packet.node.issue));
    out.push_str(&format!("- title: {}\n", packet.node.title));
    out.push_str(&format!("- role / lane: {} / {}\n", packet.node.train_role, packet.node.lane));
    out.push_str(&format!(
        "- buildable: {} (conflict key `{}`)\n",
        packet.node.buildable, packet.node.conflict_key
    ));
    out.push_str(&format!(
        "- binding: commit `{}` tree `{}`\n",
        shorten(&packet.binding.git_commit),
        shorten(&packet.binding.git_tree)
    ));
    out.push_str(&format!(
        "- digests: manifest `{}` ledger `{}` mapping `{}` input `{}`\n",
        shorten(&packet.binding.manifest_digest),
        shorten(&packet.binding.ledger_digest),
        shorten(&packet.binding.mapping_digest),
        shorten(&packet.binding.input_digest)
    ));
    if let (Some(entry), Some(kind), Some(class)) = (
        &packet.revision_currency.latest_entry_id,
        &packet.revision_currency.latest_revision_kind,
        &packet.revision_currency.latest_semantic_class,
    ) {
        out.push_str(&format!("- revision currency: {entry} ({kind} / {class})\n"));
    } else {
        out.push_str("- revision currency: no ledger entry references this node\n");
    }

    out.push_str("\n## Proposition\n\n");
    out.push_str(&format!("- one-PR outcome: {}\n", packet.proposition.one_pr_outcome));
    out.push_str(&format!("- claim ceiling: {}\n", packet.proposition.claim_ceiling));
    out.push_str(&format!(
        "- spec disposition: {} (owner {})\n",
        packet.spec.disposition, packet.spec.owner
    ));
    out.push_str(&format!("- first falsifier: {}\n", packet.spec.first_falsifier));
    out.push_str(&format!("- proof focused: {}\n", packet.spec.proof_focused));

    out.push_str("\n## Applicable instructions (ordered root to local)\n\n");
    if packet.instructions.is_empty() {
        out.push_str("- none recorded\n");
    }
    for instruction in &packet.instructions {
        out.push_str(&format!(
            "- `{}` ({}, sha256 `{}`)\n",
            instruction.path,
            instruction.scope,
            shorten(&instruction.sha256)
        ));
    }

    out.push_str("\n## Checked specs\n\n");
    if packet.checked_specs.is_empty() {
        out.push_str("- none on this tree\n");
    }
    for spec in &packet.checked_specs {
        out.push_str(&format!("- `.spec/{}` ({} file(s))\n", spec.bundle, spec.files.len()));
    }

    out.push_str("\n## Components (exact-tree navigation evidence, not scope authority)\n\n");
    if packet.components.is_empty() {
        out.push_str(&format!("- no population mapping: {}\n", gap_reason(packet)));
    }
    for component in &packet.components {
        let symbol = component
            .symbol
            .as_ref()
            .map(|symbol| {
                format!(" :: `{symbol}` ({})", component.symbol_kind.clone().unwrap_or_default())
            })
            .unwrap_or_default();
        out.push_str(&format!(
            "- {} [{}] `{}`{symbol} (sha256 `{}`)\n",
            component.component_id,
            component.role,
            component.path,
            shorten(&component.sha256)
        ));
    }

    out.push_str("\n## Tests and falsifiers\n\n");
    if packet.tests.is_empty() {
        out.push_str("- none recorded\n");
    }
    for test in &packet.tests {
        let selector =
            test.selector.as_ref().map(|selector| format!(" :: `{selector}`")).unwrap_or_default();
        out.push_str(&format!("- [{}] `{}`{selector}\n", test.kind, test.path));
    }

    if !packet.generated.is_empty() {
        out.push_str("\n## Generated surfaces (evidence only, never canonical input)\n\n");
        for generated in &packet.generated {
            out.push_str(&format!(
                "- `{}` -> `{}` by `{}`; stale check: `{}`\n",
                generated.input, generated.output, generated.generator, generated.stale_check
            ));
        }
    }

    out.push_str("\n## Minimum read set (ordered)\n\n");
    if packet.read_set.is_empty() {
        out.push_str("- none recorded\n");
    }
    for path in &packet.read_set {
        out.push_str(&format!("- `{path}`\n"));
    }

    out.push_str("\n## Expected write set (ordered, exact files only)\n\n");
    if packet.write_set.is_empty() {
        out.push_str("- none recorded\n");
    }
    for path in &packet.write_set {
        out.push_str(&format!("- `{path}`\n"));
    }

    if !packet.not_authority.is_empty() {
        out.push_str("\n## Nearby but NOT authority\n\n");
        for row in &packet.not_authority {
            out.push_str(&format!(
                "- `{}` — {} (owner {})\n",
                row.path_or_symbol, row.reason, row.owner
            ));
        }
    }

    if !packet.graph.forbidden_adjacent_owners.is_empty() {
        out.push_str("\n## Forbidden adjacent owners\n\n");
        for owner in &packet.graph.forbidden_adjacent_owners {
            out.push_str(&format!("- {owner}\n"));
        }
    }

    if !packet.gaps.is_empty() {
        out.push_str("\n## Mapping gaps (fail-closed)\n\n");
        for gap in &packet.gaps {
            out.push_str(&format!(
                "- {} — {} (owner #{}, action `{}`)\n",
                gap.subject, gap.reason, gap.owner_issue, gap.action
            ));
        }
    }

    out.push_str(&format!(
        "\n## Bounds and privacy\n\n- maxima: components {} / tests {} / read {} / write {} / \
         not-authority {}\n- privacy: repository-relative paths only, no source text, no \
         absolute paths, no logs or credentials\n",
        packet.bounds.max_components_per_node,
        packet.bounds.max_tests_per_node,
        packet.bounds.max_read_set,
        packet.bounds.max_write_set,
        packet.bounds.max_not_authority,
    ));
    out
}

fn gap_reason(packet: &NodeContextPacket) -> String {
    packet
        .gaps
        .first()
        .map(|gap| format!("{} (owner #{})", gap.reason, gap.owner_issue))
        .unwrap_or_else(|| "unpopulated".to_owned())
}

fn shorten(digest: &str) -> &str {
    digest.get(..12).unwrap_or(digest)
}
