// SPDX-License-Identifier: GPL-3.0-or-later
//! A hash whose value never changes: FNV-1a, 64 bit. For anything that is stored, compared across
//! runs or used in a file name (clipboard history, thumbnail cache). `std`'s `DefaultHasher` is not
//! guaranteed to give the same value in another Rust release. Not for security: it only tells
//! "the same content" apart from "other content" for non-adversarial data.

const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

/// Streaming FNV-1a 64.
#[derive(Clone, Copy, Debug)]
pub struct Fnv1a64(u64);

impl Default for Fnv1a64 {
    fn default() -> Self {
        Self::new()
    }
}

impl Fnv1a64 {
    pub const fn new() -> Self {
        Self(OFFSET)
    }

    pub fn write(&mut self, bytes: &[u8]) -> &mut Self {
        for b in bytes {
            self.0 ^= u64::from(*b);
            self.0 = self.0.wrapping_mul(PRIME);
        }
        self
    }

    pub const fn finish(&self) -> u64 {
        self.0
    }
}

/// Hash of several byte strings, each followed by a 0 byte so that `("ab", "c")` and `("a", "bc")`
/// differ.
pub fn fnv1a64(parts: &[&[u8]]) -> u64 {
    let mut h = Fnv1a64::new();
    for p in parts {
        h.write(p).write(&[0]);
    }
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Published FNV-1a 64 test vectors: the value must never change.
    #[test]
    fn known_vectors() {
        assert_eq!(Fnv1a64::new().finish(), 0xcbf2_9ce4_8422_2325);
        assert_eq!(Fnv1a64::new().write(b"a").finish(), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(
            Fnv1a64::new().write(b"foobar").finish(),
            0x8594_4171_f739_67e8
        );
    }

    #[test]
    fn parts_are_separated() {
        assert_ne!(fnv1a64(&[b"ab", b"c"]), fnv1a64(&[b"a", b"bc"]));
        assert_eq!(fnv1a64(&[b"x", b"y"]), fnv1a64(&[b"x", b"y"]));
    }
}
