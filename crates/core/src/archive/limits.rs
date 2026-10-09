//! The limits that keep an extraction from being a surprise.

/// Past these, the plan of an extraction carries a warning that has to be confirmed by
/// typing, because the archive may be a bomb: a few kilobytes that unpack to the whole disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArchiveLimits {
    /// Total size of what would be written.
    pub max_total_bytes: u64,
    /// Written bytes per byte of archive.
    pub max_ratio: u64,
    /// The ratio is only a concern for archives that unpack to at least this much; a tiny file
    /// of zeros is harmless.
    pub ratio_floor_bytes: u64,
    /// Members past this many make a listing stop (and say so).
    pub max_entries: usize,
}

impl Default for ArchiveLimits {
    fn default() -> Self {
        ArchiveLimits {
            max_total_bytes: 8 << 30,
            max_ratio: 200,
            ratio_floor_bytes: 256 << 20,
            max_entries: 5_000_000,
        }
    }
}

impl ArchiveLimits {
    /// The reason an extraction of `written` bytes from an archive of `packed` bytes is
    /// suspicious, or `None`.
    pub fn suspicious(&self, packed: u64, written: u64) -> Option<String> {
        if written > self.max_total_bytes {
            return Some(format!(
                "it would write {} (the limit is {})",
                crate::display::bytes(written),
                crate::display::bytes(self.max_total_bytes)
            ));
        }
        if written >= self.ratio_floor_bytes && packed > 0 && written / packed > self.max_ratio {
            return Some(format!(
                "it unpacks {} from {}, {} times larger (the limit is {} times)",
                crate::display::bytes(written),
                crate::display::bytes(packed),
                written / packed,
                self.max_ratio
            ));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tiny_file_of_zeros_is_not_a_bomb() {
        let l = ArchiveLimits::default();
        assert!(l.suspicious(1000, 10_000_000).is_none());
        assert!(l.suspicious(1 << 20, 100 << 20).is_none());
    }

    #[test]
    fn size_and_ratio_are_both_checked() {
        let l = ArchiveLimits::default();
        assert!(l.suspicious(1 << 30, 9 << 30).is_some());
        assert!(l.suspicious(1 << 20, 1 << 30).is_some()); // 1024x
        assert!(l.suspicious(1 << 30, 2 << 30).is_none());
    }
}
