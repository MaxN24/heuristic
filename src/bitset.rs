use std::fmt::Debug;

use crate::constants::MAX_N;

pub type BitStorage = u32;

/// A bitset to store up to `MAX_N` bits
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct BitSet {
    bits: BitStorage,
}

impl BitSet {
    #[inline]
    pub const fn empty() -> Self {
        BitSet { bits: 0 }
    }

    #[inline]
    pub const fn from_storage(bits: BitStorage) -> Self {
        BitSet { bits }
    }

    #[inline]
    pub const fn full_up_to(len: usize) -> Self {
        if len == 0 {
            return BitSet::empty();
        }

        if len >= BitStorage::BITS as usize {
            return BitSet::from_storage(BitStorage::MAX);
        }

        BitSet::from_storage(((1u64 << len) - 1) as BitStorage)
    }

    #[inline]
    pub const fn bits(self) -> BitStorage {
        self.bits
    }

    #[allow(unused)]
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.bits == 0
    }

    #[inline]
    pub const fn single(i: usize) -> Self {
        BitSet { bits: 1 << i }
    }

    #[inline]
    pub fn insert(&mut self, index: usize) {
        debug_assert!(index < MAX_N);
        let bit_mask = 1 << index;

        self.bits |= bit_mask;
    }

    #[inline]
    pub fn remove(&mut self, index: usize) {
        debug_assert!(index < MAX_N);
        let bit_mask = 1 << index;

        self.bits &= !bit_mask;
    }

    #[inline]
    pub const fn contains(self, index: usize) -> bool {
        debug_assert!(index < MAX_N);
        let bit_mask = 1 << index;

        (self.bits & bit_mask) != 0
    }

    #[inline]
    pub const fn union(self, other: Self) -> Self {
        BitSet {
            bits: self.bits | other.bits,
        }
    }

    #[inline]
    pub const fn intersect(self, other: Self) -> Self {
        BitSet {
            bits: self.bits & other.bits,
        }
    }

    #[inline]
    pub const fn complement(self) -> Self {
        const MASK: BitStorage = if MAX_N >= BitStorage::BITS as usize {
            BitStorage::MAX
        } else {
            ((1u64 << MAX_N) - 1) as BitStorage
        };
        BitSet {
            bits: !self.bits & MASK,
        }
    }

    #[inline]
    pub const fn is_disjoint(self, other: Self) -> bool {
        self.bits & other.bits == 0
    }

    #[inline]
    pub const fn len(self) -> usize {
        self.bits.count_ones() as usize
    }
}

impl IntoIterator for BitSet {
    type IntoIter = BitSetIter;
    type Item = usize;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        BitSetIter { bitset: self }
    }
}

impl From<BitStorage> for BitSet {
    #[inline]
    fn from(bits: BitStorage) -> Self {
        Self::from_storage(bits)
    }
}

pub struct BitSetIter {
    bitset: BitSet,
}

impl Iterator for BitSetIter {
    type Item = usize;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let next = self.bitset.bits.trailing_zeros() as usize;

        if next < MAX_N {
            // remove first set bit
            self.bitset.bits = (self.bitset.bits - 1) & self.bitset.bits;
            Some(next)
        } else {
            None
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.bitset.len(), Some(self.bitset.len()))
    }
}

impl ExactSizeIterator for BitSetIter {
    #[inline]
    fn len(&self) -> usize {
        self.bitset.len()
    }
}

impl Debug for BitSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BitSet")
            .field(
                "bits",
                &format!("{:032b}", self.bits).chars().collect::<String>(),
            )
            .field(
                "set_bits",
                &(0..MAX_N).filter(|i| self.contains(*i)).collect::<Vec<_>>(),
            )
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_up_to_handles_32_bits() {
        let full = BitSet::full_up_to(MAX_N);
        assert_eq!(full.bits(), BitStorage::MAX);
    }
}
