#!/usr/bin/env python3
"""Make the provisional #4997 authority generation ABA-safe.

Runs after patch_4997_current.py and before the count-checked source editor.
The unavailable state retains its generation so disable/enable cannot recreate
an authority identity previously captured by a backend Arc.
"""

from pathlib import Path

path = Path("scripts/maintenance/apply_4997_ai_activation_authority.py")
text = path.read_text(encoding="utf-8")

old_enum = '''#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum AiActivationAuthority {
    /// No independently admitted user/operator activation exists.
    #[default]
    Unavailable,
'''
new_enum = '''#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AiActivationAuthority {
    /// No independently admitted user/operator activation exists.
    ///
    /// The generation is retained across disablement so a later enable cannot
    /// recreate an authority identity captured by a stale backend Arc.
    Unavailable { generation: u64 },
'''
assert text.count(old_enum) == 1, text.count(old_enum)
text = text.replace(old_enum, new_enum)

impl_marker = '''impl AiActivationAuthority {
'''
default_impl = '''impl Default for AiActivationAuthority {
    fn default() -> Self {
        Self::Unavailable { generation: 0 }
    }
}

'''
assert text.count(impl_marker) == 1, text.count(impl_marker)
text = text.replace(impl_marker, default_impl + impl_marker)

assert text.count('Self::Unavailable => "unavailable",') == 1
text = text.replace(
    'Self::Unavailable => "unavailable",',
    'Self::Unavailable { .. } => "unavailable",',
)
assert text.count('Self::Unavailable => 0,') == 1
text = text.replace(
    'Self::Unavailable => 0,',
    'Self::Unavailable { generation } => generation,',
)

constructor_old = '''    '                super::AiActivationAuthority::Unavailable,\\n'
'''
constructor_new = '''    '                super::AiActivationAuthority::default(),\\n'
'''
assert text.count(constructor_old) == 1, text.count(constructor_old)
text = text.replace(constructor_old, constructor_new)

body_old = '''        let mut authority = self.ai_activation_authority.lock();
        let next_generation = match *authority {
            super::AiActivationAuthority::Unavailable => 1,
            super::AiActivationAuthority::TrustedUserOperator { generation, .. } => {
                generation.saturating_add(1)
            }
        };
        *authority = if enabled {
            super::AiActivationAuthority::TrustedUserOperator {
                adapter: "expose_lsp_test_api",
                generation: next_generation,
            }
        } else {
            super::AiActivationAuthority::Unavailable
        };
'''
body_new = '''        let mut authority = self.ai_activation_authority.lock();
        let next_generation = authority.generation().saturating_add(1);
        *authority = if enabled {
            super::AiActivationAuthority::TrustedUserOperator {
                adapter: "expose_lsp_test_api",
                generation: next_generation,
            }
        } else {
            super::AiActivationAuthority::Unavailable {
                generation: next_generation,
            }
        };
'''
assert text.count(body_old) == 1, text.count(body_old)
text = text.replace(body_old, body_new)

path.write_text(text, encoding="utf-8")
