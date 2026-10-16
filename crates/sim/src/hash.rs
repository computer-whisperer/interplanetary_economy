//! A fixed hash (M1b.md, step 1 plan, part 4): design keys, keyed draws'
//! seeds and state hashes are the same in every build and on every
//! machine, which std's hashers do not promise. FNV-1a over the bytes
//! written, finished by SplitMix64's mixer.

use std::hash::Hasher;

#[derive(Clone, Copy, Debug)]
pub struct Fixed(u64);

impl Default for Fixed {
    fn default() -> Fixed {
        Fixed(0xcbf2_9ce4_8422_2325)
    }
}

impl Hasher for Fixed {
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn finish(&self) -> u64 {
        let mut z = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
}

impl Fixed {
    pub fn u64(&mut self, x: u64) -> &mut Fixed {
        self.write(&x.to_le_bytes());
        self
    }

    /// By its bits: -0 and 0 differ, as they do to a design.
    pub fn f64(&mut self, x: f64) -> &mut Fixed {
        self.u64(x.to_bits())
    }

    pub fn str(&mut self, s: &str) -> &mut Fixed {
        self.u64(s.len() as u64);
        self.write(s.as_bytes());
        self
    }

    /// By its `Debug` text, which tells apart any two values that differ
    /// (floats print to the bit), for values with no `Hash`.
    pub fn debug(&mut self, x: &impl std::fmt::Debug) -> &mut Fixed {
        self.str(&format!("{x:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same in every build: these values are part of the format
    /// (computed apart, in Python).
    #[test]
    fn it_is_fixed() {
        assert_eq!(Fixed::default().finish(), 0xc381_7c01_6ba4_ff30);
        assert_eq!(Fixed::default().u64(1).finish(), 0xc2be_3627_c2bf_e353);
        assert_ne!(
            Fixed::default().f64(0.0).finish(),
            Fixed::default().f64(-0.0).finish()
        );
        // A string's length goes first: ("ab", "c") is not ("a", "bc").
        assert_ne!(
            Fixed::default().str("ab").str("c").finish(),
            Fixed::default().str("a").str("bc").finish()
        );
    }
}
