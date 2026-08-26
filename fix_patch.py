import io

# F. navigation: clamp canonical offsets against the CURRENT document text
# length before pos16 conversion (raced didChange safety).
path = 'crates/perl-lsp-rs/src/runtime/language/navigation.rs'
src = io.open(path, encoding='utf-8').read()
old = '''                    let ((sl, sc), (el, ec)) = {
                        let documents = self.documents_guard();
                        self.get_document(&documents, uri)
                            .map(|doc| {
                                (
                                    self.offset_to_pos16(doc, usize::try_from(start).unwrap_or(0)),
                                    self.offset_to_pos16(doc, usize::try_from(end).unwrap_or(0)),
                                )
                            })
                            .unwrap_or(((0, 0), (0, 0)))
                    };'''
new = '''                    let ((sl, sc), (el, ec)) = {
                        let documents = self.documents_guard();
                        self.get_document(&documents, uri)
                            .map(|doc| {
                                // Clamp against the CURRENT text: a didChange
                                // racing the released lock must never push a
                                // stale snapshot's byte offsets out of range.
                                let text_len = doc.text.len();
                                let clamp =
                                    |value: u32| usize::try_from(value).unwrap_or(0).min(text_len);
                                (
                                    self.offset_to_pos16(doc, clamp(start)),
                                    self.offset_to_pos16(doc, clamp(end)),
                                )
                            })
                            .unwrap_or(((0, 0), (0, 0)))
                    };'''
assert old in src, 'nav clamp anchor missing'
src = src.replace(old, new, 1)
io.open(path, 'w', encoding='utf-8', newline='').write(src)
print('F done')

# G. workspace: hook vs route labels, remaining-budget cap, stale-fallback
# entries, documentation whitespace.
path = 'crates/perl-lsp-rs/src/runtime/workspace.rs'
src = io.open(path, encoding='utf-8').read()

old_ent_start = src.find('                let package = entity')
old_ent_end = src.find('                    container_name: Some(package),')
assert old_ent_start != -1 and old_ent_end > old_ent_start
segment = src[old_ent_start:old_ent_end]
new_segment = segment.replace(
    'name: format!("{} {DANCER2_ROUTE_LABEL}", entity.bare_name),',
    'let label = if entity.is_route { DANCER2_ROUTE_LABEL } else { DANCER2_HOOK_LABEL };\n                    name: format!("{} {label}", entity.bare_name),')
new_segment = new_segment.replace(
    '''documentation: Some(
                        "Canonical Dancer2 framework projection anchored to the source \\
                         declaration; virtual entry, no generated body"
                            .to_string(),
                    ),''',
    '''documentation: Some(
                        "Canonical Dancer2 framework projection anchored to the source \
                         declaration; virtual entry, no generated body"
                            .to_string(),
                    ),''')
src = src[:old_ent_start] + new_segment + src[old_ent_end:]
src = src.replace(
    'use perl_lsp_rs_core::providers::dancer2::{DANCER2_ROUTE_LABEL, dancer2_workspace_entities};',
    'use perl_lsp_rs_core::providers::dancer2::{\n    DANCER2_HOOK_LABEL, DANCER2_ROUTE_LABEL, dancer2_workspace_entities,\n};')

old_stale = '''            if self.workspace_index_stale_for_any_open_document() {
                tracing::debug!(
                    query,
                    "Workspace symbol: skipping stale workspace index tier, using open-doc fallback"
                );
                return self.search_open_documents_for_symbols(query, cap);
            }'''
new_stale = '''            if self.workspace_index_stale_for_any_open_document() {
                tracing::debug!(
                    query,
                    "Workspace symbol: skipping stale workspace index tier, using open-doc fallback"
                );
                // The canonical Dancer2 entries are computed from open
                // documents' current snapshots, so they stay available on
                // the stale-index fallback path (#8928).
                let dancer2_entries = self.dancer2_workspace_symbols_typed(query, cap);
                let fallback = self.search_open_documents_for_symbols(query, cap)?;
                let merged = match fallback {
                    Some(Value::Array(mut items)) => {
                        for entry in dancer2_entries {
                            if let Ok(value) = serde_json::to_value(&entry)
                                && items.len() < cap
                            {
                                items.push(value);
                            }
                        }
                        Some(Value::Array(items))
                    }
                    other => other,
                };
                return Ok(merged);
            }'''
assert old_stale in src, 'stale anchor missing'
src = src.replace(old_stale, new_stale, 1)

old_ext = '''                    // Canonical Dancer2 route entries (#8928): labeled
                    // framework projections from open-document canonical
                    // facts, bounded by the same cap.
                    symbols.extend(dancer2_entries);'''
new_ext = '''                    // Canonical Dancer2 route/hook entries (#8928): labeled
                    // framework projections from open-document canonical
                    // facts, bounded by the remaining page budget so the
                    // response never exceeds `cap`.
                    let remaining_budget = cap.saturating_sub(symbols.len());
                    symbols.extend(dancer2_entries.into_iter().take(remaining_budget));'''
assert old_ext in src, 'extend anchor missing'
src = src.replace(old_ext, new_ext, 1)
io.open(path, 'w', encoding='utf-8', newline='').write(src)
print('G done')

# G4: core symbols entities carry is_route.
path = 'crates/perl-lsp-rs-core/src/providers/dancer2/symbols.rs'
src = io.open(path, encoding='utf-8').read()
Q3 = chr(39) * 3
anchor = Q3 + '''    /// Declaration span end (byte offset).
    pub end: u32,
    /// Whether the underlying fact was exact.
    pub exact: bool,
}
''' + Q3
assert anchor in src, 'entity struct anchor missing'
src = src.replace(anchor, Q3 + '''    /// Declaration span end (byte offset).
    pub end: u32,
    /// Whether the underlying fact was exact.
    pub exact: bool,
    /// `true` for route entities, `false` for hook entities (label provenance).
    pub is_route: bool,
}
''' + Q3, 1)

route_push = Q3 + '''            start: route.envelope.anchor.start_byte,
            end: route.envelope.anchor.end_byte,
            exact: route.status() == perl_semantic_facts::SemanticFactStatus::Exact,
        });
''' + Q3
assert route_push in src, 'route push anchor missing'
src = src.replace(route_push, Q3 + '''            start: route.envelope.anchor.start_byte,
            end: route.envelope.anchor.end_byte,
            exact: route.status() == perl_semantic_facts::SemanticFactStatus::Exact,
            is_route: true,
        });
''' + Q3, 1)

hook_push = Q3 + '''                start: hook.envelope.anchor.start_byte,
                end: hook.envelope.anchor.end_byte,
                exact: hook.status() == perl_semantic_facts::SemanticFactStatus::Exact,
            });
''' + Q3
assert hook_push in src, 'hook push anchor missing'
src = src.replace(hook_push, Q3 + '''                start: hook.envelope.anchor.start_byte,
                end: hook.envelope.anchor.end_byte,
                exact: hook.status() == perl_semantic_facts::SemanticFactStatus::Exact,
                is_route: false,
            });
''' + Q3, 1)
io.open(path, 'w', encoding='utf-8', newline='').write(src)
print('G4 done')
