//! Visible (non-pending) lexical bindings for one [`super::Scope`].
//!
//! Ordinary same-scope lookup, unused reporting, and declaration history live
//! here so [`super::Scope::declare_variable_parts`] can keep statement-modifier
//! pending maps (#14840 / #1772) on a separate path.
//!
//! `latest` is the active slot consulted by lookup. `history` retains earlier
//! same-scope declarations so their unused/shadowing records are not dropped
//! when a later binding becomes active (#15056). The active slot is replaced
//! only when the incoming declaration is textually later (or first); `our` /
//! `local` skip pending maps, so modifier analysis can install a later binding
//! before an earlier one.

use super::index_to_sigil;
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;

/// Per-sigil map of the currently active lexical slot.
pub(super) type VariableMaps = [Option<FxHashMap<String, Rc<Variable>>>; 6];
/// Per-sigil, per-name ordered history of every visible lexical declaration.
pub(super) type BindingHistoryMaps = [Option<FxHashMap<String, Vec<Rc<Variable>>>>; 6];

#[derive(Debug)]
pub(super) struct Variable {
    pub(super) declaration_offset: usize,
    pub(super) is_used: RefCell<bool>,
    pub(super) is_our: bool,
    pub(super) is_initialized: RefCell<bool>,
}

#[derive(Debug)]
pub(super) struct VisibleBindings {
    latest: RefCell<VariableMaps>,
    history: RefCell<BindingHistoryMaps>,
}

impl VisibleBindings {
    pub(super) fn empty() -> Self {
        Self {
            latest: RefCell::new(std::array::from_fn(|_| None)),
            history: RefCell::new(std::array::from_fn(|_| None)),
        }
    }

    pub(super) fn latest_offset(&self, idx: usize, name: &str) -> Option<usize> {
        self.latest.borrow()[idx]
            .as_ref()
            .and_then(|map| map.get(name))
            .map(|var| var.declaration_offset)
    }

    pub(super) fn get(&self, idx: usize, name: &str) -> Option<Rc<Variable>> {
        self.latest.borrow()[idx].as_ref().and_then(|map| map.get(name)).cloned()
    }

    pub(super) fn contains(&self, idx: usize, name: &str) -> bool {
        self.latest.borrow()[idx].as_ref().is_some_and(|map| map.contains_key(name))
    }

    /// Install `variable` into history, and into `latest` when `replace_latest`.
    ///
    /// `replace_latest` is the textual later-wins guard: `our`/`local` skip the
    /// pending path, so a statement-modifier condition (analyzed first, textually
    /// later) must not be overwritten by the earlier statement declaration.
    pub(super) fn declare(
        &self,
        idx: usize,
        name: &str,
        variable: Rc<Variable>,
        replace_latest: bool,
    ) {
        if replace_latest {
            let mut latest = self.latest.borrow_mut();
            latest[idx]
                .get_or_insert_with(FxHashMap::default)
                .insert(name.to_string(), variable.clone());
        }
        {
            let mut history = self.history.borrow_mut();
            history[idx]
                .get_or_insert_with(FxHashMap::default)
                .entry(name.to_string())
                .or_default()
                .push(variable);
        }
    }

    /// Merge statement-modifier pending slots into the visible table without
    /// changing pending-map insertion rules.
    pub(super) fn absorb_pending(
        &self,
        pending_latest: &mut VariableMaps,
        pending_history: &mut BindingHistoryMaps,
    ) {
        let mut latest = self.latest.borrow_mut();
        for (visible, deferred) in latest.iter_mut().zip(pending_latest.iter_mut()) {
            if let Some(declarations) = deferred.take() {
                visible.get_or_insert_with(FxHashMap::default).extend(declarations);
            }
        }
        drop(latest);
        let mut visible_history = self.history.borrow_mut();
        for (visible_h, pending_h) in visible_history.iter_mut().zip(pending_history.iter_mut()) {
            if let Some(entries_by_name) = pending_h.take() {
                let visible_slot = visible_h.get_or_insert_with(FxHashMap::default);
                for (name, mut entries) in entries_by_name {
                    visible_slot.entry(name).or_default().append(&mut entries);
                }
            }
        }
    }

    /// Report unused diagnostics for the latest binding only. Earlier same-scope
    /// declarations stay in `history` (so their unused metadata is not dropped)
    /// but are not double-warned; `VariableRedeclaration` already covers them.
    pub(super) fn for_each_reportable_unused<F>(&self, mut f: F)
    where
        F: FnMut(String, usize),
    {
        for (idx, inner_opt) in self.latest.borrow().iter().enumerate() {
            if let Some(inner) = inner_opt {
                for (name, var) in inner {
                    if !*var.is_used.borrow() && !var.is_our {
                        if name.starts_with('_') {
                            continue;
                        }
                        // Auto-suppress unused $self in plain subs — it's the
                        // dominant Moose/Moo invocant idiom and flagging it is
                        // more noisy than useful (#5060 item 3).
                        if name == "self" && idx == 0 {
                            continue;
                        }
                        let full_name = format!("{}{}", index_to_sigil(idx), name);
                        f(full_name, var.declaration_offset);
                    }
                }
            }
        }
    }
}
