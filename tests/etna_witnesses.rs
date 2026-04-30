//! Witness tests for the lz4_flex ETNA workload.
//!
//! Each `witness_*` test calls one of the `property_*` functions in
//! `lz4_flex::etna` with frozen inputs. Tests pass on the base commit and
//! fail when the corresponding mutation is active (marauders or patch).

use lz4_flex::etna::{
    property_decompress_short_input_no_panic, property_dont_panic_on_drop,
    property_small_dict_no_panic, PropertyResult,
};

fn assert_pass(r: PropertyResult) {
    match r {
        PropertyResult::Pass => {}
        PropertyResult::Fail(m) => panic!("property failed: {m}"),
        PropertyResult::Discard => panic!("property unexpectedly discarded"),
    }
}

// ---- dont_panic_on_drop_44e14b1_1 ----

#[test]
fn witness_dont_panic_on_drop_case_small_payload() {
    let data = b"hello world".to_vec();
    assert_pass(property_dont_panic_on_drop(data));
}

#[test]
fn witness_dont_panic_on_drop_case_empty_payload() {
    assert_pass(property_dont_panic_on_drop(Vec::new()));
}

#[test]
fn witness_dont_panic_on_drop_case_repeated_bytes() {
    let data = vec![0xABu8; 256];
    assert_pass(property_dont_panic_on_drop(data));
}

// ---- small_dict_no_panic_2d83a3d_1 ----

#[test]
fn witness_small_dict_no_panic_case_three_byte_dict() {
    // Dict shorter than MINMATCH (4) used to panic.
    let input = vec![10u8, 12, 14, 16, 18, 10, 12, 14, 16, 18, 10, 12, 14, 16, 18, 10, 12, 14, 16, 18];
    let dict = vec![10u8, 12, 14];
    assert_pass(property_small_dict_no_panic((input, dict)));
}

#[test]
fn witness_small_dict_no_panic_case_two_byte_dict() {
    let input = vec![0u8; 64];
    let dict = vec![0u8, 1];
    assert_pass(property_small_dict_no_panic((input, dict)));
}

#[test]
fn witness_small_dict_no_panic_case_one_byte_dict() {
    let input = vec![1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
    let dict = vec![42u8];
    assert_pass(property_small_dict_no_panic((input, dict)));
}

// ---- decompress_short_input_no_panic_e0e7e5c_1 ----

#[test]
fn witness_decompress_short_input_no_panic_case_empty() {
    assert_pass(property_decompress_short_input_no_panic(Vec::new()));
}

#[test]
fn witness_decompress_short_input_no_panic_case_one_byte() {
    assert_pass(property_decompress_short_input_no_panic(vec![0u8]));
}

#[test]
fn witness_decompress_short_input_no_panic_case_three_bytes() {
    assert_pass(property_decompress_short_input_no_panic(vec![0u8, 1, 2]));
}
