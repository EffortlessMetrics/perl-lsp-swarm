//! Lightweight replacement back end for the `idna` crate, used via
//! `[patch.crates-io]` to keep ICU4X data tables (several MiB of compiled
//! Unicode data) out of the size-capped perllsp binary.
//!
//! Behavior contract:
//! - ASCII domain labels take the exact same path as the ICU back end
//!   (lowercasing, LDH validation happens in `idna` itself). This covers
//!   `file://` URIs (empty/localhost hosts) and ordinary DNS names, which
//!   are the only hosts the LSP transport ever constructs.
//! - Non-ASCII labels get a best-effort approximation: full Unicode
//!   lowercasing, approximate Mark/Virama/joining classification from
//!   compact range tables, and a permissive bidi classification. Strict
//!   UTS46 normalization (NFC with full mapping tables) is NOT performed,
//!   so exotic internationalized hostnames in http(s) document links may
//!   be rejected or encoded differently than with the ICU back end.
//!   The full (non-capped) build is unaffected only if it also resolves
//!   this patch; see the workspace root Cargo.toml note.

#![no_std]

/// Joining_Type values needed by the `idna` crate's checks.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
enum Jt {
    NonJoining = 0,
    Transparent = 1,
    DualJoining = 2,
    LeftJoining = 3,
    RightJoining = 4,
    JoinCausing = 5,
}

const fn jt_mask(jt: Jt) -> u32 {
    1u32 << (jt as u32)
}

/// Mask for checking for both left and dual joining.
pub const LEFT_OR_DUAL_JOINING_MASK: JoiningTypeMask =
    JoiningTypeMask(jt_mask(Jt::LeftJoining) | jt_mask(Jt::DualJoining));

/// Mask for checking for both left and dual joining.
pub const RIGHT_OR_DUAL_JOINING_MASK: JoiningTypeMask =
    JoiningTypeMask(jt_mask(Jt::RightJoining) | jt_mask(Jt::DualJoining));

/// Value for the Joining_Type Unicode property.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct JoiningType(Jt);

impl JoiningType {
    /// Returns the corresponding `JoiningTypeMask`.
    #[inline(always)]
    pub fn to_mask(self) -> JoiningTypeMask {
        JoiningTypeMask(jt_mask(self.0))
    }

    /// `true` iff this value is the Transparent value.
    #[inline(always)]
    pub fn is_transparent(self) -> bool {
        self.0 == Jt::Transparent
    }
}

/// A mask representing potentially multiple `JoiningType` values.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct JoiningTypeMask(u32);

impl JoiningTypeMask {
    /// `true` iff both masks have at least one `JoiningType` in common.
    #[inline(always)]
    pub fn intersects(self, other: JoiningTypeMask) -> bool {
        self.0 & other.0 != 0
    }
}

/// Bidi_Class values distinguished by the masks below.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
enum Bc {
    LeftToRight = 0,
    RightToLeft = 1,
    ArabicLetter = 2,
    EuropeanNumber = 3,
    ArabicNumber = 4,
    EuropeanSeparator = 5,
    CommonSeparator = 6,
    EuropeanTerminator = 7,
    OtherNeutral = 8,
    BoundaryNeutral = 9,
    NonspacingMark = 10,
    ParagraphSeparator = 11,
    SegmentSeparator = 12,
    WhiteSpace = 13,
    LeftToRightEmbedding = 14,
    RightToLeftEmbedding = 15,
}

const fn bc_mask(bc: Bc) -> u32 {
    1u32 << (bc as u32)
}

/// Mask for checking if the domain is a bidi domain.
pub const RTL_MASK: BidiClassMask = BidiClassMask(
    bc_mask(Bc::RightToLeft) | bc_mask(Bc::ArabicLetter) | bc_mask(Bc::ArabicNumber),
);

/// Mask for allowable bidi classes in the first character of a label.
pub const FIRST_BC_MASK: BidiClassMask = BidiClassMask(
    bc_mask(Bc::LeftToRight) | bc_mask(Bc::RightToLeft) | bc_mask(Bc::ArabicLetter),
);

/// Mask for allowable bidi classes of the last character in an LTR label.
pub const LAST_LTR_MASK: BidiClassMask =
    BidiClassMask(bc_mask(Bc::LeftToRight) | bc_mask(Bc::EuropeanNumber));

/// Mask for allowable bidi classes of the last character in an RTL label.
pub const LAST_RTL_MASK: BidiClassMask = BidiClassMask(
    bc_mask(Bc::RightToLeft)
        | bc_mask(Bc::ArabicLetter)
        | bc_mask(Bc::EuropeanNumber)
        | bc_mask(Bc::ArabicNumber),
);

/// Mask for allowable bidi classes of the middle characters in an LTR label.
pub const MIDDLE_LTR_MASK: BidiClassMask = BidiClassMask(
    bc_mask(Bc::LeftToRight)
        | bc_mask(Bc::EuropeanNumber)
        | bc_mask(Bc::EuropeanSeparator)
        | bc_mask(Bc::CommonSeparator)
        | bc_mask(Bc::EuropeanTerminator)
        | bc_mask(Bc::OtherNeutral)
        | bc_mask(Bc::BoundaryNeutral)
        | bc_mask(Bc::NonspacingMark),
);

/// Mask for allowable bidi classes of the middle characters in an RTL label.
pub const MIDDLE_RTL_MASK: BidiClassMask = BidiClassMask(
    bc_mask(Bc::RightToLeft)
        | bc_mask(Bc::ArabicLetter)
        | bc_mask(Bc::EuropeanNumber)
        | bc_mask(Bc::EuropeanSeparator)
        | bc_mask(Bc::CommonSeparator)
        | bc_mask(Bc::EuropeanTerminator)
        | bc_mask(Bc::OtherNeutral)
        | bc_mask(Bc::BoundaryNeutral)
        | bc_mask(Bc::NonspacingMark),
);

/// Value for the Bidi_Class Unicode property.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct BidiClass(Bc);

impl BidiClass {
    /// Returns the corresponding `BidiClassMask`.
    #[inline(always)]
    pub fn to_mask(self) -> BidiClassMask {
        BidiClassMask(bc_mask(self.0))
    }

    /// `true` iff this value is Left_To_Right.
    #[inline(always)]
    pub fn is_ltr(self) -> bool {
        self.0 == Bc::LeftToRight
    }

    /// `true` iff this value is Nonspacing_Mark.
    #[inline(always)]
    pub fn is_nonspacing_mark(self) -> bool {
        self.0 == Bc::NonspacingMark
    }

    /// `true` iff this value is European_Number.
    #[inline(always)]
    pub fn is_european_number(self) -> bool {
        self.0 == Bc::EuropeanNumber
    }

    /// `true` iff this value is Arabic_Number.
    #[inline(always)]
    pub fn is_arabic_number(self) -> bool {
        self.0 == Bc::ArabicNumber
    }
}

/// A mask representing potentially multiple `BidiClass` values.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct BidiClassMask(u32);

impl BidiClassMask {
    /// `true` iff both masks have at least one `BidiClass` in common.
    #[inline(always)]
    pub fn intersects(self, other: BidiClassMask) -> bool {
        self.0 & other.0 != 0
    }
}

/// Compact combining-mark ranges (General_Category Mn/Me plus common Mc
/// blocks) for `is_mark` and transparent-joining approximation.
const MARK_RANGES: &[(u32, u32)] = &[
    (0x0300, 0x036F),
    (0x0483, 0x0489),
    (0x0591, 0x05BD),
    (0x05BF, 0x05C2),
    (0x064B, 0x0652),
    (0x0670, 0x0670),
    (0x06D6, 0x06ED),
    (0x0711, 0x0711),
    (0x0730, 0x074A),
    (0x07A6, 0x07B0),
    (0x0816, 0x082B),
    (0x0859, 0x085B),
    (0x08D4, 0x0902),
    (0x093A, 0x093A),
    (0x093C, 0x093C),
    (0x0941, 0x0948),
    (0x094D, 0x094D),
    (0x0951, 0x0957),
    (0x0962, 0x0963),
    (0x0981, 0x0981),
    (0x09BC, 0x09BC),
    (0x09C1, 0x09C4),
    (0x09CD, 0x09CD),
    (0x09E2, 0x09E3),
    (0x0A01, 0x0A02),
    (0x0A3C, 0x0A3C),
    (0x0A41, 0x0A42),
    (0x0A47, 0x0A48),
    (0x0A4B, 0x0A4D),
    (0x0A51, 0x0A51),
    (0x0A70, 0x0A71),
    (0x0A75, 0x0A75),
    (0x0A81, 0x0A82),
    (0x0ABC, 0x0ABC),
    (0x0AC1, 0x0AC5),
    (0x0AC7, 0x0AC8),
    (0x0ACD, 0x0ACD),
    (0x0AE2, 0x0AE3),
    (0x0B01, 0x0B01),
    (0x0B3C, 0x0B3C),
    (0x0B3F, 0x0B3F),
    (0x0B41, 0x0B44),
    (0x0B4D, 0x0B4D),
    (0x0B56, 0x0B56),
    (0x0B62, 0x0B63),
    (0x0B82, 0x0B82),
    (0x0BC0, 0x0BC0),
    (0x0BCD, 0x0BCD),
    (0x0C3D, 0x0C3E),
    (0x0C40, 0x0C40),
    (0x0C46, 0x0C48),
    (0x0C4A, 0x0C4D),
    (0x0C55, 0x0C56),
    (0x0C62, 0x0C63),
    (0x0C81, 0x0C81),
    (0x0CBC, 0x0CBC),
    (0x0CBF, 0x0CBF),
    (0x0CC6, 0x0CC6),
    (0x0CCC, 0x0CCD),
    (0x0CE2, 0x0CE3),
    (0x0D01, 0x0D01),
    (0x0D41, 0x0D44),
    (0x0D4D, 0x0D4D),
    (0x0D62, 0x0D63),
    (0x0DCA, 0x0DCA),
    (0x0DD2, 0x0DD4),
    (0x0DD6, 0x0DD6),
    (0x0E31, 0x0E31),
    (0x0E34, 0x0E3A),
    (0x0E47, 0x0E4E),
    (0x0EB1, 0x0EB1),
    (0x0EB4, 0x0EBC),
    (0x0EC8, 0x0ECE),
    (0x0F18, 0x0F19),
    (0x0F35, 0x0F35),
    (0x0F37, 0x0F37),
    (0x0F39, 0x0F39),
    (0x0F71, 0x0F84),
    (0x0F86, 0x0F87),
    (0x0F8D, 0x0F97),
    (0x0F99, 0x0FBC),
    (0x0FC6, 0xFC6F),
    (0x102D, 0x1030),
    (0x1032, 0x1037),
    (0x1039, 0x103A),
    (0x103D, 0x103E),
    (0x1058, 0x1059),
    (0x105E, 0x1060),
    (0x1071, 0x1074),
    (0x1082, 0x1082),
    (0x1085, 0x1086),
    (0x108D, 0x108D),
    (0x109D, 0x109D),
    (0x1160, 0x11FF),
    (0x135D, 0x135F),
    (0x1712, 0x1714),
    (0x1732, 0x1734),
    (0x1752, 0x1753),
    (0x1772, 0x1773),
    (0x17B4, 0x17B5),
    (0x17B7, 0x17BD),
    (0x17C6, 0x17C6),
    (0x17C9, 0x17D3),
    (0x17DD, 0x17DD),
    (0x180B, 0x180E),
    (0x18A9, 0x18A9),
    (0x1920, 0x1922),
    (0x1927, 0x1928),
    (0x1932, 0x1932),
    (0x1939, 0x193B),
    (0x1A17, 0x1A18),
    (0x1A1B, 0x1A1B),
    (0x1A56, 0x1A56),
    (0x1A58, 0x1A5E),
    (0x1A60, 0x1A60),
    (0x1A62, 0x1A62),
    (0x1A65, 0x1A6C),
    (0x1A73, 0x1A7C),
    (0x1A7F, 0x1A7F),
    (0x1AB0, 0x1ABE),
    (0x1B00, 0x1B03),
    (0x1B34, 0x1B34),
    (0x1B36, 0x1B3A),
    (0x1B3C, 0x1B3C),
    (0x1B42, 0x1B42),
    (0x1B6B, 0x1B73),
    (0x1B80, 0x1B81),
    (0x1BA2, 0x1BA5),
    (0x1BA8, 0x1BA9),
    (0x1BAB, 0x1BAD),
    (0x1BE6, 0x1BE6),
    (0x1BE8, 0x1BE9),
    (0x1BED, 0x1BED),
    (0x1BEF, 0x1BF1),
    (0x1C2C, 0x1C33),
    (0x1C36, 0x1C37),
    (0x1CD0, 0x1CD2),
    (0x1CD4, 0x1CE0),
    (0x1CE2, 0x1CE8),
    (0x1CED, 0x1CED),
    (0x1CF4, 0x1CF4),
    (0x1CF8, 0x1CF9),
    (0x1DC0, 0x1DF5),
    (0x1DFC, 0x1DFF),
    (0x200C, 0x200D),
    (0x20D0, 0x20F0),
    (0x2CEF, 0x2CF1),
    (0x2D7F, 0x2D7F),
    (0x2DE0, 0x2DFF),
    (0x302A, 0x302D),
    (0x3099, 0x309A),
    (0x309A, 0x309A),
    (0xA66F, 0xA672),
    (0xA674, 0xA67D),
    (0xA69E, 0xA69F),
    (0xA6F0, 0xA6F1),
    (0xA802, 0xA802),
    (0xA806, 0xA806),
    (0xA80B, 0xA80B),
    (0xA825, 0xA826),
    (0xA8C4, 0xA8C4),
    (0xA8E0, 0xA8F1),
    (0xA926, 0xA92D),
    (0xA947, 0xA951),
    (0xA980, 0xA982),
    (0xA9B3, 0xA9B3),
    (0xA9B6, 0xA9B9),
    (0xA9BC, 0xA9BC),
    (0xA9E5, 0xA9E5),
    (0xAA29, 0xAA2E),
    (0xAA31, 0xAA32),
    (0xAA35, 0xAA36),
    (0xAA43, 0xAA43),
    (0xAA4C, 0xAA4C),
    (0xAA7C, 0xAA7C),
    (0xAAB0, 0xAAB0),
    (0xAAB2, 0xAAB4),
    (0xAAB7, 0xAAB8),
    (0xAABE, 0xAABF),
    (0xAAC1, 0xAAC1),
    (0xAAEC, 0xAAED),
    (0xAAF6, 0xAAF6),
    (0xABE5, 0xABE5),
    (0xABE8, 0xABE8),
    (0xABED, 0xABED),
    (0xFB1E, 0xFB1E),
    (0xFE00, 0xFE0F),
    (0xFE20, 0xFE2F),
    (0x101FD, 0x101FD),
    (0x102E0, 0x102E0),
    (0x10376, 0x1037A),
    (0x10A01, 0x10A03),
    (0x10A05, 0x10A06),
    (0x10A0C, 0x10A0F),
    (0x10A38, 0x10A3A),
    (0x10A3F, 0x10A3F),
    (0x10AE5, 0x10AE6),
    (0x10EFD, 0x10EFF),
    (0x10F46, 0x10F50),
    (0x11001, 0x11001),
    (0x11038, 0x11046),
    (0x11070, 0x11070),
    (0x11073, 0x11074),
    (0x1107F, 0x11081),
    (0x110B3, 0x110B6),
    (0x110B9, 0x110BA),
    (0x110C2, 0x110C2),
    (0x11100, 0x11102),
    (0x11127, 0x1112B),
    (0x1112D, 0x11134),
    (0x11173, 0x11173),
    (0x11180, 0x11181),
    (0x111B6, 0x111BE),
    (0x111CA, 0x111CC),
    (0x1122F, 0x11231),
    (0x11234, 0x11234),
    (0x11236, 0x11237),
    (0x112DF, 0x112DF),
    (0x112E3, 0x112EA),
    (0x11300, 0x11301),
    (0x1133C, 0x1133C),
    (0x11340, 0x11340),
    (0x11366, 0x1136C),
    (0x11370, 0x11374),
    (0x11438, 0x1143F),
    (0x11442, 0x11444),
    (0x11446, 0x11446),
    (0x114B3, 0x114B8),
    (0x114BA, 0x114BA),
    (0x114BF, 0x114C0),
    (0x114C2, 0x114C3),
    (0x115B2, 0x115B5),
    (0x115BC, 0x115BD),
    (0x115BF, 0x115C0),
    (0x115DC, 0x115DD),
    (0x11633, 0x1163A),
    (0x1163D, 0x1163D),
    (0x1163F, 0x11640),
    (0x116AB, 0x116AB),
    (0x116AD, 0x116AD),
    (0x116B0, 0x116B5),
    (0x116B7, 0x116B7),
    (0x1171D, 0x1171F),
    (0x11722, 0x11725),
    (0x11727, 0x1172B),
    (0x16AF0, 0x16AF4),
    (0x16B30, 0x16B36),
    (0x16F51, 0x16F7E),
    (0x16F8F, 0x16F92),
    (0x16FE4, 0x16FE4),
    (0x1BC9D, 0x1BC9E),
    (0x1D167, 0x1D169),
    (0x1D17B, 0x1D182),
    (0x1D185, 0x1D18B),
    (0x1D1AA, 0x1D1AD),
    (0x1D242, 0x1D244),
    (0x1DA00, 0x1DA36),
    (0x1DA3B, 0x1DA6C),
    (0x1DA75, 0x1DA75),
    (0x1DA84, 0x1DA84),
    (0x1DA9B, 0x1DA9F),
    (0x1DAA1, 0x1DAAF),
    (0x1E000, 0x1E006),
    (0x1E008, 0x1E018),
    (0x1E01B, 0x1E021),
    (0x1E023, 0x1E024),
    (0x1E026, 0x1E02A),
    (0x1E130, 0x1E136),
    (0x1E2AE, 0x1E2AE),
    (0x1E2EC, 0x1E2EF),
    (0x1E8D0, 0x1E8D6),
    (0x1E944, 0x1E94A),
    (0xE0100, 0xE01EF),
];

fn in_ranges(ranges: &[(u32, u32)], c: char) -> bool {
    let cp = c as u32;
    ranges.iter().any(|&(lo, hi)| cp >= lo && cp <= hi)
}

/// Common virama code points (Canonical_Combining_Class = 9).
const VIRAMA: &[u32] = &[
    0x094D, 0x09CD, 0x0A4D, 0x0ACD, 0x0B4D, 0x0BCD, 0x0C4D, 0x0CCD, 0x0D4D, 0x0DCA, 0x1039, 0x103A,
    0x1714, 0x1734, 0x17D1, 0x17D3, 0x1A55, 0x1B45, 0x1BA8, 0x1BF3, 0x2D7F, 0xA98B, 0xA9BF, 0xAAC0,
    0xAAF5, 0x11046, 0x11070, 0x110B9, 0x111C0, 0x11241, 0x11442, 0x114C2, 0x115BF, 0x116B6,
    0x11722, 0x11A47, 0x11A99, 0x11C3F, 0x11C92, 0x11D44, 0x11D97, 0x16AF4, 0x1ABD, 0x1B80,
];

fn bidi_of(c: char) -> Bc {
    let cp = c as u32;
    match cp {
        0x0041..=0x005A | 0x0061..=0x007A | 0x00C0..=0x02FF | 0x0370..=0x058F | 0x0900..=0x1FFF
        | 0x3041..=0x30FF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF => Bc::LeftToRight,
        0x0030..=0x0039 => Bc::EuropeanNumber,
        0x0590..=0x05FF => Bc::RightToLeft,
        0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF | 0xFB50..=0xFDFF
        | 0xFE70..=0xFEFF => {
            if matches!(cp, 0x0660..=0x0669 | 0x06F0..=0x06F9) {
                Bc::ArabicNumber
            } else if c.is_alphabetic() {
                Bc::ArabicLetter
            } else {
                Bc::OtherNeutral
            }
        }
        _ if in_ranges(MARK_RANGES, c) => Bc::NonspacingMark,
        _ if c.is_alphabetic() => Bc::LeftToRight,
        _ if c.is_numeric() => Bc::EuropeanNumber,
        _ => Bc::OtherNeutral,
    }
}

/// An adapter between a Unicode back end and the `idna` crate.
pub struct Adapter;

impl Default for Adapter {
    fn default() -> Self {
        Self::new()
    }
}

impl Adapter {
    /// Constructor (no compiled data in this back end).
    #[inline(always)]
    pub const fn new() -> Self {
        Self
    }

    /// `true` iff the Canonical_Combining_Class of `c` is Virama.
    #[inline(always)]
    pub fn is_virama(&self, c: char) -> bool {
        VIRAMA.contains(&(c as u32))
    }

    /// `true` iff `c` is a combining mark (approximate range table).
    #[inline(always)]
    pub fn is_mark(&self, c: char) -> bool {
        in_ranges(MARK_RANGES, c)
    }

    /// Returns the Bidi_Class of `c` (approximate for non-ASCII).
    #[inline(always)]
    pub fn bidi_class(&self, c: char) -> BidiClass {
        BidiClass(bidi_of(c))
    }

    /// Returns the Joining_Type of `c` (Transparent for marks, else
    /// Non_Joining; Arabic joining forms are approximated as Non_Joining,
    /// which can only reject exotic joined labels, never corrupt ASCII).
    #[inline(always)]
    pub fn joining_type(&self, c: char) -> JoiningType {
        if in_ranges(MARK_RANGES, c) {
            JoiningType(Jt::Transparent)
        } else {
            JoiningType(Jt::NonJoining)
        }
    }

    /// UTS46 map + normalize approximation: full lowercasing only.
    #[inline(always)]
    pub fn map_normalize<'delegate, I: Iterator<Item = char> + 'delegate>(
        &'delegate self,
        iter: I,
    ) -> impl Iterator<Item = char> + 'delegate {
        iter.flat_map(char::to_lowercase)
    }

    /// UTS46 normalize + validate approximation: full lowercasing only.
    #[inline(always)]
    pub fn normalize_validate<'delegate, I: Iterator<Item = char> + 'delegate>(
        &'delegate self,
        iter: I,
    ) -> impl Iterator<Item = char> + 'delegate {
        iter.flat_map(char::to_lowercase)
    }
}
