use crate::*;

use self::checkpoint_context::build_checkpoint_context;
use self::checkpoint_restore::restore_format_mode_context;

mod checkpoint_context;
mod checkpoint_restore;

impl Checkpointable for PerlLexer<'_> {
    fn checkpoint(&self) -> LexerCheckpoint {
        LexerCheckpoint {
            position: self.position,
            mode: self.mode,
            delimiter_stack: self.delimiter_stack.clone(),
            in_prototype: self.in_prototype,
            prototype_depth: self.prototype_depth,
            after_sub: self.after_sub,
            after_arrow: self.after_arrow,
            hash_brace_depth: self.hash_brace_depth,
            after_var_subscript: self.after_var_subscript,
            paren_depth: self.paren_depth,
            current_pos: self.current_pos,
            eof_emitted: self.eof_emitted,
            context: build_checkpoint_context(self),
        }
    }

    fn restore(&mut self, checkpoint: &LexerCheckpoint) {
        self.position = checkpoint.position;
        self.mode = checkpoint.mode;
        self.delimiter_stack.clone_from(&checkpoint.delimiter_stack);
        self.in_prototype = checkpoint.in_prototype;
        self.prototype_depth = checkpoint.prototype_depth;
        self.after_sub = checkpoint.after_sub;
        self.after_arrow = checkpoint.after_arrow;
        self.hash_brace_depth = checkpoint.hash_brace_depth;
        self.after_var_subscript = checkpoint.after_var_subscript;
        self.paren_depth = checkpoint.paren_depth;
        self.current_pos = checkpoint.current_pos;
        self.eof_emitted = checkpoint.eof_emitted;

        restore_format_mode_context(self, checkpoint);
    }

    fn can_restore(&self, checkpoint: &LexerCheckpoint) -> bool {
        // Can restore if the position is valid for our input
        checkpoint.position <= self.input.len()
    }
}
