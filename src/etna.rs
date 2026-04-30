//! ETNA benchmark harness for the `lz4_flex` crate.
//!
//! Each `property_*` function below is a framework-neutral, deterministic
//! invariant check used by `src/bin/etna.rs` and the witness tests under
//! `tests/etna_witnesses.rs`. They take owned concrete inputs, return
//! `PropertyResult`, and never panic on the property side: panics from the
//! library-under-test are caught and surfaced as `Fail`.

use std::format;
use std::io::Write;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::string::String;
use std::vec::Vec;

use crate::block::{
    compress_prepend_size_with_dict, decompress_size_prepended,
    decompress_size_prepended_with_dict,
};
use crate::frame::FrameEncoder;

#[allow(unused_imports)]
use crate::block::DecompressError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropertyResult {
    Pass,
    Fail(String),
    Discard,
}

// ---------------------------------------------------------------------------
// Bug 1: AutoFinishEncoder panics on drop when the underlying writer errors.
// (44e14b1 — Don't panic on drop)
// ---------------------------------------------------------------------------

/// A `Write` impl that always errors on `flush` and `write`. Used to provoke
/// the buggy `Drop` impl into panicking. The errors carry no inner payload so
/// they survive the `From<io::Error> for frame::Error` conversion intact.
struct AlwaysErrorWriter;

impl Write for AlwaysErrorWriter {
    fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
    }
}

/// Dropping an `AutoFinishEncoder` must never panic, even when the underlying
/// writer returns errors during the implicit `try_finish()`.
///
/// The buggy `Drop` impl calls `panic!("Error when flushing frame on drop")`
/// when `try_finish()` returns `Err`. The fix swallows the error.
pub fn property_dont_panic_on_drop(data: Vec<u8>) -> PropertyResult {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let writer = AlwaysErrorWriter;
        let mut auto = FrameEncoder::new(writer).auto_finish();
        // Best-effort write; the writer will error but we don't care here.
        let _ = auto.write_all(&data);
        // `auto` drops at end of scope — that's the surface under test.
    }));
    match result {
        Ok(()) => PropertyResult::Pass,
        Err(_) => PropertyResult::Fail(format!(
            "AutoFinishEncoder drop panicked with {} input bytes",
            data.len()
        )),
    }
}

// ---------------------------------------------------------------------------
// Bug 2: compress_with_dict panics when the dict is shorter than 4 bytes.
// (2d83a3d — Fix: Small dict leads to panic)
// ---------------------------------------------------------------------------

/// `compress_prepend_size_with_dict(input, dict)` followed by
/// `decompress_size_prepended_with_dict(.., dict)` must not panic and must
/// roundtrip the input exactly, regardless of the dict length (in particular
/// when `dict.len() < 4`).
///
/// The buggy implementation reads `dict_data` as `&[u8]` and indexes the
/// hash table without checking that the dict is at least 4 bytes long; a
/// short dict causes an out-of-bounds slice read which panics.
pub fn property_small_dict_no_panic(args: (Vec<u8>, Vec<u8>)) -> PropertyResult {
    let (input, dict) = args;
    let result = catch_unwind(AssertUnwindSafe(|| {
        let compressed = compress_prepend_size_with_dict(&input, &dict);
        decompress_size_prepended_with_dict(&compressed, &dict)
    }));
    match result {
        Err(_) => PropertyResult::Fail(format!(
            "compress_with_dict/decompress_with_dict panicked: input.len()={}, dict.len()={}",
            input.len(),
            dict.len()
        )),
        Ok(Err(e)) => PropertyResult::Fail(format!(
            "decompress_size_prepended_with_dict returned Err({e:?}): input.len()={}, dict.len()={}",
            input.len(),
            dict.len()
        )),
        Ok(Ok(decoded)) => {
            if decoded == input {
                PropertyResult::Pass
            } else {
                PropertyResult::Fail(format!(
                    "roundtrip mismatch: orig.len()={}, decoded.len()={}",
                    input.len(),
                    decoded.len()
                ))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Bug 3: decompress_size_prepended panics when input shorter than 4 bytes.
// (e0e7e5c — Don't panic passing an empty buffer to decompress_size_prepended)
// ---------------------------------------------------------------------------

/// `decompress_size_prepended(input)` must return `Err(_)` (never panic) when
/// `input.len() < 4`. The fix routes the read through `uncompressed_size`
/// which returns `Err(ExpectedAnotherByte)`. The buggy version indexed
/// `input[0..=3]` directly, panicking on an empty/short slice.
pub fn property_decompress_short_input_no_panic(input: Vec<u8>) -> PropertyResult {
    if input.len() >= 4 {
        return PropertyResult::Discard;
    }
    let result = catch_unwind(AssertUnwindSafe(|| decompress_size_prepended(&input)));
    match result {
        Err(_) => PropertyResult::Fail(format!(
            "decompress_size_prepended panicked on short input.len()={}",
            input.len()
        )),
        Ok(Ok(_)) => PropertyResult::Fail(format!(
            "decompress_size_prepended returned Ok on short input.len()={} (expected Err)",
            input.len()
        )),
        Ok(Err(_)) => PropertyResult::Pass,
    }
}

