//! Scope-aware class grammar context for the declaration parser (#10740).
//!
//! Perl's native `class` feature makes a small number of constructs
//! grammatically admissible only while the parser is inside class grammar.
//! On the current tree that is exactly one construct: an `ADJUST { ... }`
//! block, which outside class grammar remains ordinary `ADJUST` followed by a
//! block expression. `field` and `method` are *not* gated on this context —
//! they are admitted by token lookahead alone — so this context governs ADJUST
//! admission only.
//!
//! This module owns **grammar admission**, not class semantics. It carries no
//! class name, entity, scope, inheritance, field storage, generation, or object
//! identity, and nothing here may become a semantic ownership signal. Semantic
//! class lifetime belongs to the downstream owners (#10346 / #6672).
//!
//! It replaces a scalar `in_class_body: usize` counter whose correctness
//! depended on every writer pairing `+= 1` with a matching `-= 1` on every exit
//! path. Restoration here is expressed as "return to the depth I observed on
//! entry", which cannot leak a frame, cannot underflow, and does not require
//! callers to remember a paired reset.

/// The source form of an active class grammar frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClassGrammarForm {
    /// Block form: `class Foo { ... }`.
    ///
    /// Active from the body's opening `{` until its matching `}`, including
    /// when the body is closed by error recovery or truncated input.
    Block,

    /// Statement form: `class Foo;`.
    ///
    /// Active from the accepted terminator until the next `class` or
    /// `package`, the end of the enclosing statement list, or EOF.
    ///
    /// The form is represented here so that activating statement-form classes
    /// (#10864) is a transition on this state machine rather than a redesign of
    /// parser state. Production parsing does not enter this form yet: on the
    /// current tree `class Foo;` is still a parse error, and #10740 explicitly
    /// does not admit it.
    #[allow(dead_code, reason = "entered by #10864; see module docs")]
    Statement,
}

/// A caller's observation of the context depth at a point in the parse.
///
/// Restoring a mark returns the context to exactly the state the caller
/// observed, regardless of how many frames the intervening parse entered or
/// failed to leave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClassGrammarMark(usize);

/// Parser-owned, scope-aware class grammar context.
///
/// A stack of frames rather than a counter, so that the active *form* is
/// inspectable (block vs statement) and restoration can target an exact prior
/// depth. Frames nest: a class declared inside another class body pushes a
/// second frame and popping it restores the enclosing one.
#[derive(Debug, Default)]
pub(crate) struct ClassGrammarContext {
    frames: Vec<ClassGrammarForm>,
}

impl ClassGrammarContext {
    /// Whether class-only member syntax is currently admissible.
    ///
    /// True while any frame is active. This is the single admission predicate
    /// the statement parser consults.
    pub(crate) fn admits_class_members(&self) -> bool {
        !self.frames.is_empty()
    }

    /// The innermost active form, or `None` outside class grammar.
    #[allow(dead_code, reason = "boundary selector for #10864; see module docs")]
    pub(crate) fn current_form(&self) -> Option<ClassGrammarForm> {
        self.frames.last().copied()
    }

    /// Observe the current depth so it can be restored later.
    pub(crate) fn mark(&self) -> ClassGrammarMark {
        ClassGrammarMark(self.frames.len())
    }

    /// Enter a class grammar frame of `form`.
    pub(crate) fn enter(&mut self, form: ClassGrammarForm) {
        self.frames.push(form);
    }

    /// Restore the context to a previously observed `mark`.
    ///
    /// Truncating to the observed depth is deliberately tolerant: it discards
    /// any frames an inner production entered and did not leave, and it is a
    /// no-op when the context is already at or below the mark. A balanced
    /// decrement could not offer either property.
    pub(crate) fn restore(&mut self, mark: ClassGrammarMark) {
        self.frames.truncate(mark.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_context_admits_no_class_members() {
        let context = ClassGrammarContext::default();
        assert!(!context.admits_class_members());
        assert_eq!(context.current_form(), None);
    }

    #[test]
    fn entering_a_block_frame_admits_class_members() {
        let mut context = ClassGrammarContext::default();
        context.enter(ClassGrammarForm::Block);
        assert!(context.admits_class_members());
        assert_eq!(context.current_form(), Some(ClassGrammarForm::Block));
    }

    #[test]
    fn restoring_a_mark_returns_to_the_observed_state() {
        let mut context = ClassGrammarContext::default();
        let outside = context.mark();
        context.enter(ClassGrammarForm::Block);
        assert!(context.admits_class_members());
        context.restore(outside);
        assert!(!context.admits_class_members());
    }

    #[test]
    fn nested_frames_restore_the_exact_enclosing_form() {
        let mut context = ClassGrammarContext::default();
        context.enter(ClassGrammarForm::Statement);
        let inside_statement = context.mark();

        context.enter(ClassGrammarForm::Block);
        assert_eq!(context.current_form(), Some(ClassGrammarForm::Block));

        context.restore(inside_statement);
        assert_eq!(
            context.current_form(),
            Some(ClassGrammarForm::Statement),
            "leaving an inner block must restore the enclosing statement frame, not clear it"
        );
    }

    #[test]
    fn restoring_discards_frames_an_inner_production_left_behind() {
        let mut context = ClassGrammarContext::default();
        let outside = context.mark();

        // Simulates an inner production that entered frames and returned early
        // without leaving them: restoration must still be exact.
        context.enter(ClassGrammarForm::Block);
        context.enter(ClassGrammarForm::Block);
        context.enter(ClassGrammarForm::Statement);

        context.restore(outside);
        assert!(
            !context.admits_class_members(),
            "restoring a mark must discard every frame entered after it"
        );
    }

    #[test]
    fn restoring_a_stale_mark_cannot_underflow() {
        let mut context = ClassGrammarContext::default();
        context.enter(ClassGrammarForm::Block);
        let deep = context.mark();
        context.restore(ClassGrammarMark(0));

        // Restoring to a deeper mark than the current depth is inert rather
        // than a panic or a wraparound, which a `usize` counter could not do.
        context.restore(deep);
        assert!(!context.admits_class_members());
    }

    #[test]
    fn statement_and_block_forms_are_distinct() {
        assert_ne!(ClassGrammarForm::Block, ClassGrammarForm::Statement);
    }
}
