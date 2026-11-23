//! Bloom filter for basename double-spend tracking.
//!
//! Matches the JavaCard implementation in BloomFilter.java:
//! - 1200-byte bit array (9585 usable bits)
//! - 7 hash functions derived from SHA-256
//! - Tracks basenames seen within the current epoch
//! - Resets on epoch transition (monotonic epoch counter)
//!
//! The bloom filter is the client-side defense against double-spending.
//! If a basename has been seen before in this epoch, the card refuses to
//! sign — preventing the same token from being spent twice. The network
//! performs the authoritative check via basename linkability.
//!
//! False positive rate: ~1% at 1000 entries (7 hash functions, 9585 bits).
//! This means ~1% of legitimate unique basenames will be falsely rejected.
//! Acceptable for a client-side defense-in-depth measure.

use sha2::{Digest, Sha256};

/// Bloom filter size in bytes (matches JavaCard: 1200 bytes).
const BLOOM_SIZE: usize = 1200;
/// Number of usable bits.
const BLOOM_BITS: usize = BLOOM_SIZE * 8; // 9600
/// Number of hash functions (k=7 is optimal for 1000 elements in 9600 bits).
const NUM_HASHES: usize = 7;

pub struct BloomFilter {
    /// Bit array.
    bits: [u8; BLOOM_SIZE],
    /// Current epoch number (monotonic).
    epoch: u32,
    /// Number of elements added in this epoch.
    count: u32,
}

impl BloomFilter {
    pub fn new() -> Self {
        Self {
            bits: [0u8; BLOOM_SIZE],
            epoch: 0,
            count: 0,
        }
    }

    /// Check if a basename is in the filter. If not, add it.
    ///
    /// Returns `true` if the basename was ALREADY present (likely double-spend).
    /// Returns `false` if the basename is new (added to filter).
    pub fn check_and_add(&mut self, basename: &[u8]) -> bool {
        let hashes = self.compute_hashes(basename);

        // Check if all bits are set (element likely present).
        let mut all_set = true;
        for &h in &hashes {
            let byte_idx = (h / 8) as usize;
            let bit_idx = (h % 8) as u8;
            if byte_idx < BLOOM_SIZE {
                if self.bits[byte_idx] & (1 << bit_idx) == 0 {
                    all_set = false;
                }
            }
        }

        if all_set {
            return true; // Duplicate (or false positive).
        }

        // Add element: set all bits.
        for &h in &hashes {
            let byte_idx = (h / 8) as usize;
            let bit_idx = (h % 8) as u8;
            if byte_idx < BLOOM_SIZE {
                self.bits[byte_idx] |= 1 << bit_idx;
            }
        }

        self.count += 1;
        false
    }

    /// Check if a basename is in the filter without adding it.
    pub fn contains(&self, basename: &[u8]) -> bool {
        let hashes = self.compute_hashes(basename);
        for &h in &hashes {
            let byte_idx = (h / 8) as usize;
            let bit_idx = (h % 8) as u8;
            if byte_idx < BLOOM_SIZE {
                if self.bits[byte_idx] & (1 << bit_idx) == 0 {
                    return false;
                }
            }
        }
        true
    }

    /// Reset the bloom filter for a new epoch.
    ///
    /// Epoch must be strictly increasing (monotonic).
    /// Returns false if the new epoch is not greater than the current epoch.
    pub fn reset_for_epoch(&mut self, new_epoch: u32) -> bool {
        if new_epoch <= self.epoch {
            defmt::warn!(
                "Bloom: rejecting epoch reset {} -> {} (not monotonic)",
                self.epoch,
                new_epoch
            );
            return false;
        }
        self.bits.fill(0);
        self.epoch = new_epoch;
        self.count = 0;
        defmt::info!("Bloom: reset for epoch {}", new_epoch);
        true
    }

    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    pub fn count(&self) -> u32 {
        self.count
    }

    /// Compute k hash indices from SHA-256 of the basename.
    ///
    /// Uses the double-hashing technique: h(i) = (h1 + i*h2) mod m
    /// where h1 and h2 are derived from the first 4 bytes of SHA-256.
    ///
    /// This matches the JavaCard implementation which takes 2 bytes per
    /// hash function from the SHA-256 output (14 bytes total for 7 hashes).
    fn compute_hashes(&self, basename: &[u8]) -> [u16; NUM_HASHES] {
        let hash = Sha256::digest(basename);
        let mut result = [0u16; NUM_HASHES];

        // Extract 7 hash indices from SHA-256 output.
        // Each index is 2 bytes (big-endian), taken mod BLOOM_BITS.
        for i in 0..NUM_HASHES {
            let offset = i * 2;
            let raw = u16::from_be_bytes([hash[offset], hash[offset + 1]]);
            result[i] = raw % BLOOM_BITS as u16;
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bloom_basic() {
        let mut bloom = BloomFilter::new();
        bloom.reset_for_epoch(1);

        // First insertion should return false (not present).
        assert!(!bloom.check_and_add(b"basename_1"));
        assert_eq!(bloom.count(), 1);

        // Second insertion of same basename should return true (duplicate).
        assert!(bloom.check_and_add(b"basename_1"));

        // Different basename should return false.
        assert!(!bloom.check_and_add(b"basename_2"));
        assert_eq!(bloom.count(), 2);
    }

    #[test]
    fn test_bloom_epoch_reset() {
        let mut bloom = BloomFilter::new();
        bloom.reset_for_epoch(1);
        bloom.check_and_add(b"test");

        // Can't go backwards.
        assert!(!bloom.reset_for_epoch(0));
        assert!(!bloom.reset_for_epoch(1));

        // Forward reset clears the filter.
        assert!(bloom.reset_for_epoch(2));
        assert_eq!(bloom.count(), 0);
        assert!(!bloom.check_and_add(b"test")); // Previously seen, now cleared.
    }
}
