//! Structural checks of the generated partition and anchor tables that need no
//! specification text, plus a pinned digest so an accidental edit cannot pass.
//!
//! The differential tests in `bc7_spec.rs` and `bc6h_spec.rs` share these tables
//! with the decoder, so on their own they would not notice a wrong row.
#[path = "../src/tables.rs"]
#[allow(dead_code)]
#[rustfmt::skip]
mod tables;
use tables::{ANCHOR2, ANCHOR3_1, ANCHOR3_2, BC6H_MODE_NUMBERS, PARTITION2, PARTITION3};

#[test]
fn every_partition_uses_all_its_subsets_and_starts_in_subset_zero() {
    for (subsets, table) in [(2u8, &PARTITION2), (3, &PARTITION3)] {
        for (index, row) in table.iter().enumerate() {
            assert_eq!(row[0], 0, "{subsets}-subset partition {index}");
            assert!(row.iter().all(|s| *s < subsets), "partition {index}");
            for subset in 0..subsets {
                assert!(row.contains(&subset), "partition {index} subset {subset}");
            }
        }
    }
}

#[test]
fn every_anchor_lies_in_the_subset_it_anchors() {
    for i in 0..64 {
        assert_eq!(PARTITION2[i][ANCHOR2[i] as usize], 1, "two subsets, {i}");
        assert_eq!(
            PARTITION3[i][ANCHOR3_1[i] as usize], 1,
            "three subsets, {i}"
        );
        assert_eq!(
            PARTITION3[i][ANCHOR3_2[i] as usize], 2,
            "three subsets, {i}"
        );
    }
}

#[test]
fn bc6h_mode_numbers_are_the_fourteen_defined_header_values() {
    let mut seen = BC6H_MODE_NUMBERS;
    seen.sort_unstable();
    assert_eq!(seen, [0, 1, 2, 3, 6, 7, 10, 11, 14, 15, 18, 22, 26, 30]);
}

#[test]
fn tables_match_their_pinned_digest() {
    // FNV-1a over the table bytes in declaration order. Regenerating the tables
    // from the specification must reproduce this value; a hand edit will not.
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut feed = |bytes: &[u8]| {
        for b in bytes {
            hash = (hash ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for row in PARTITION2.iter().chain(PARTITION3.iter()) {
        feed(row);
    }
    feed(&ANCHOR2);
    feed(&ANCHOR3_1);
    feed(&ANCHOR3_2);
    feed(&BC6H_MODE_NUMBERS);
    assert_eq!(hash, 0xaec3_c321_8e2e_8c19, "table digest {hash:#018x}");
}
