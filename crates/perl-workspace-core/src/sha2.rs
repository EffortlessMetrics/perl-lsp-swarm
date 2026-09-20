//! Minimal, dependency-free SHA-256 primitive for durable internal identities.
//!
//! This module intentionally exposes only the small API shape needed by
//! `test_item`: incremental byte collection followed by one SHA-256 digest. It
//! is not a general cryptography facade and does not implement authentication,
//! signatures, password hashing, or secret-dependent operations.

/// Trait-shaped API used by the TestItem identity builder.
pub(crate) trait Digest {
    /// Final digest output.
    type Output;

    /// Start a fresh digest.
    fn new() -> Self
    where
        Self: Sized;

    /// Append bytes to the digest input.
    fn update(&mut self, data: impl AsRef<[u8]>);

    /// Consume the builder and produce the digest.
    fn finalize(self) -> Self::Output;
}

/// SHA-256 digest builder.
pub(crate) struct Sha256 {
    input: Vec<u8>,
}

/// Fixed SHA-256 output.
pub(crate) struct Output([u8; 32]);

impl Output {
    /// Borrow the 32 digest bytes.
    pub(crate) const fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

impl Digest for Sha256 {
    type Output = Output;

    fn new() -> Self {
        Self { input: Vec::new() }
    }

    fn update(&mut self, data: impl AsRef<[u8]>) {
        self.input.extend_from_slice(data.as_ref());
    }

    fn finalize(self) -> Self::Output {
        Output(sha256(self.input.as_slice()))
    }
}

const INITIAL_STATE: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

const ROUND_CONSTANTS: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

fn sha256(input: &[u8]) -> [u8; 32] {
    let input_len = u64::try_from(input.len()).unwrap_or(u64::MAX);
    let bit_len = input_len.saturating_mul(8);
    let mut message = Vec::with_capacity(input.len().saturating_add(72));
    message.extend_from_slice(input);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    let mut state = INITIAL_STATE;
    for block in message.chunks_exact(64) {
        compress(&mut state, block);
    }

    let mut output = [0u8; 32];
    for (chunk, word) in output.chunks_exact_mut(4).zip(state) {
        chunk.copy_from_slice(&word.to_be_bytes());
    }
    output
}

fn compress(state: &mut [u32; 8], block: &[u8]) {
    let mut schedule = [0u32; 64];
    for (index, word) in schedule.iter_mut().take(16).enumerate() {
        let offset = index * 4;
        *word = u32::from_be_bytes([
            block[offset],
            block[offset + 1],
            block[offset + 2],
            block[offset + 3],
        ]);
    }
    for index in 16..64 {
        let small_sigma_0 = schedule[index - 15].rotate_right(7)
            ^ schedule[index - 15].rotate_right(18)
            ^ (schedule[index - 15] >> 3);
        let small_sigma_1 = schedule[index - 2].rotate_right(17)
            ^ schedule[index - 2].rotate_right(19)
            ^ (schedule[index - 2] >> 10);
        schedule[index] = schedule[index - 16]
            .wrapping_add(small_sigma_0)
            .wrapping_add(schedule[index - 7])
            .wrapping_add(small_sigma_1);
    }

    let mut a = state[0];
    let mut b = state[1];
    let mut c = state[2];
    let mut d = state[3];
    let mut e = state[4];
    let mut f = state[5];
    let mut g = state[6];
    let mut h = state[7];

    for (word, round_constant) in schedule.into_iter().zip(ROUND_CONSTANTS) {
        let big_sigma_1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let choose = (e & f) ^ ((!e) & g);
        let temporary_1 = h
            .wrapping_add(big_sigma_1)
            .wrapping_add(choose)
            .wrapping_add(round_constant)
            .wrapping_add(word);
        let big_sigma_0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let majority = (a & b) ^ (a & c) ^ (b & c);
        let temporary_2 = big_sigma_0.wrapping_add(majority);

        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(temporary_1);
        d = c;
        c = b;
        b = a;
        a = temporary_1.wrapping_add(temporary_2);
    }

    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest_hex(input: &[u8]) -> String {
        let mut output = String::with_capacity(64);
        for byte in sha256(input) {
            use std::fmt::Write as _;
            let _ = write!(output, "{byte:02x}");
        }
        output
    }

    #[test]
    fn matches_fips_180_4_empty_vector() {
        assert_eq!(
            digest_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn matches_fips_180_4_abc_vector() {
        assert_eq!(
            digest_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn incremental_builder_matches_single_input() {
        let mut builder = Sha256::new();
        builder.update(b"a");
        builder.update(b"bc");
        assert_eq!(builder.finalize().as_slice(), sha256(b"abc"));
    }
}
