//! Data-file pages and their header (TS-05, TS-12).
//!
//! Every page is [`PAGE_SIZE`] bytes and starts with this header, integers
//! little-endian:
//!
//! | Offset | Size | Field |
//! |---|---|---|
//! | 0 | 4 | CRC-32C of the page number, then bytes 4..8192 |
//! | 4 | 8 | page LSN: the WAL position of the last change |
//! | 12 | 8 | tree identifier (0 for the tree directory and non-tree pages) |
//! | 20 | 1 | page type ([`PageType`]) |
//! | 21 | 3 | zero |
//!
//! The checksum covers the whole page after itself, so a torn or flipped
//! byte anywhere in the page is detected on read. It is computed over the
//! page number, as 8 little-endian bytes, followed by bytes 4 to 8 191
//! (TS-12): a valid image of page A read at page B's position fails B's
//! checksum, so a misdirected or stale write is detected. PostgreSQL binds
//! the block number into its page checksum the same way, by mixing it into
//! the checksum (R-PG `src/include/storage/checksum_impl.h:172, 193`). The
//! page number itself is not stored (TS-05).

/// Page size in bytes, fixed at creation (TS-05).
pub(crate) const PAGE_SIZE: usize = 8192;

/// [`PAGE_SIZE`] as `u64`, for offsets.
pub(crate) const PAGE_BYTES: u64 = PAGE_SIZE as u64;

/// Bytes of the page header; the page body starts here.
pub(crate) const HEADER_LEN: usize = 24;

const CRC: std::ops::Range<usize> = 0..4;
const LSN: std::ops::Range<usize> = 4..12;
const TREE: std::ops::Range<usize> = 12..20;
const TYPE: usize = 20;
/// Where the tree identifier, and after it everything a page copy keeps,
/// starts.
pub(crate) const TREE_START: usize = TREE.start;

/// The kind of a page (TS-05). The value 0 is not a type, so a page of
/// zeros never passes as a typed page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum PageType {
    /// Page 0 of `data-<g>`.
    Header = 1,
    /// A B+tree leaf.
    Leaf = 2,
    /// A B+tree inner page.
    Inner = 3,
    /// A page of an overflow chain.
    Overflow = 4,
    /// A trunk page of the persisted free list.
    FreeListTrunk = 5,
}

impl PageType {
    fn from_u8(v: u8) -> Option<PageType> {
        Some(match v {
            1 => PageType::Header,
            2 => PageType::Leaf,
            3 => PageType::Inner,
            4 => PageType::Overflow,
            5 => PageType::FreeListTrunk,
            _ => return None,
        })
    }
}

fn u64_at(page: &[u8], r: std::ops::Range<usize>) -> u64 {
    u64::from_le_bytes(page[r].try_into().expect("8-byte field"))
}

/// Zeroes `page` except its page LSN, and writes a header. The checksum is
/// not set; see [`seal`]. The page LSN is kept because a writer sets it
/// before it rewrites a page, in the same latched write (TS-09): a rewrite
/// must not lower it, or eviction could write the page before its WAL
/// group is durable.
pub(crate) fn init(page: &mut [u8], page_type: PageType, tree: u64) {
    debug_assert_eq!(page.len(), PAGE_SIZE);
    page[CRC].fill(0);
    page[TREE.start..].fill(0);
    page[TREE].copy_from_slice(&tree.to_le_bytes());
    page[TYPE] = page_type as u8;
}

/// The checksum `page` should carry at page number `page_no` (TS-12).
fn compute(page: &[u8], page_no: u64) -> u32 {
    let crc = crc32c::crc32c(&page_no.to_le_bytes());
    crc32c::crc32c_append(crc, &page[CRC.end..])
}

/// Stores the checksum of `page`, to be written at page number `page_no`,
/// in its header. Called on a copy just before the page is written
/// (TS-12).
pub(crate) fn seal(page: &mut [u8], page_no: u64) {
    debug_assert_eq!(page.len(), PAGE_SIZE);
    let crc = compute(page, page_no);
    page[CRC].copy_from_slice(&crc.to_le_bytes());
}

/// Whether the stored checksum of `page`, read at page number `page_no`,
/// matches its content and position.
#[must_use]
pub(crate) fn verify(page: &[u8], page_no: u64) -> bool {
    debug_assert_eq!(page.len(), PAGE_SIZE);
    u32::from_le_bytes(page[CRC].try_into().expect("4-byte field")) == compute(page, page_no)
}

/// The page LSN.
#[must_use]
pub(crate) fn lsn(page: &[u8]) -> u64 {
    u64_at(page, LSN)
}

/// Sets the page LSN.
pub(crate) fn set_lsn(page: &mut [u8], lsn: u64) {
    page[LSN].copy_from_slice(&lsn.to_le_bytes());
}

/// The tree identifier.
#[must_use]
pub(crate) fn tree(page: &[u8]) -> u64 {
    u64_at(page, TREE)
}

/// The page type, or `None` if the byte holds no known type.
#[must_use]
pub(crate) fn page_type(page: &[u8]) -> Option<PageType> {
    PageType::from_u8(page[TYPE])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The page number `sample` is sealed for.
    const SAMPLE_PAGE: u64 = 5;

    fn sample() -> Vec<u8> {
        let mut p = vec![0xEEu8; PAGE_SIZE];
        init(&mut p, PageType::Leaf, 42);
        set_lsn(&mut p, 7);
        p[HEADER_LEN..HEADER_LEN + 5].copy_from_slice(b"hello");
        seal(&mut p, SAMPLE_PAGE);
        p
    }

    #[test]
    fn header_round_trip() {
        let p = sample();
        assert!(verify(&p, SAMPLE_PAGE));
        assert_eq!(lsn(&p), 7);
        assert_eq!(tree(&p), 42);
        assert_eq!(page_type(&p), Some(PageType::Leaf));
        assert!(p[21..24].iter().all(|&b| b == 0));
        assert!(p[HEADER_LEN + 5..].iter().all(|&b| b == 0));
    }

    #[test]
    fn every_single_bit_flip_is_detected() {
        let p = sample();
        // Every bit of the header and a stride of the body.
        let bits = (0..HEADER_LEN * 8).chain((HEADER_LEN * 8..PAGE_SIZE * 8).step_by(61));
        for bit in bits {
            let mut q = p.clone();
            q[bit / 8] ^= 1 << (bit % 8);
            assert!(!verify(&q, SAMPLE_PAGE), "flip of bit {bit} not detected");
        }
    }

    #[test]
    fn zero_page_fails_verification_and_has_no_type() {
        let z = vec![0u8; PAGE_SIZE];
        assert!((0..64).all(|n| !verify(&z, n)));
        assert_eq!(page_type(&z), None);
    }

    #[test]
    fn init_keeps_the_page_lsn_and_clears_the_rest() {
        let mut p = vec![0xEEu8; PAGE_SIZE];
        set_lsn(&mut p, 99);
        init(&mut p, PageType::Inner, 3);
        assert_eq!(lsn(&p), 99);
        assert_eq!(p[CRC], [0; 4]);
        assert_eq!(tree(&p), 3);
        assert_eq!(page_type(&p), Some(PageType::Inner));
        assert!(p[TYPE + 1..].iter().all(|&b| b == 0));
    }

    #[test]
    fn changes_after_seal_need_a_new_seal() {
        let mut p = sample();
        set_lsn(&mut p, 8);
        assert!(!verify(&p, SAMPLE_PAGE));
        seal(&mut p, SAMPLE_PAGE);
        assert!(verify(&p, SAMPLE_PAGE));
    }

    /// TS-12: the checksum is bound to the page number, so a valid image is
    /// rejected at every other position, including a page number that
    /// differs in one bit of any of its 8 bytes.
    #[test]
    fn a_valid_image_fails_at_any_other_page_number() {
        let p = sample();
        assert!(verify(&p, SAMPLE_PAGE));
        for bit in 0..64 {
            let other = SAMPLE_PAGE ^ (1 << bit);
            assert!(
                !verify(&p, other),
                "page {other} accepted page {SAMPLE_PAGE}'s image"
            );
        }
        assert!(
            (0..10_000)
                .filter(|&n| n != SAMPLE_PAGE)
                .all(|n| !verify(&p, n))
        );
    }
}
