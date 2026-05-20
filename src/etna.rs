//! ETNA benchmark harness for the `lz4_flex` crate.
//!
//! Each `property_*` function below is a framework-neutral, deterministic
//! invariant check used by `src/bin/etna.rs` and the witness tests under
//! `tests/etna_witnesses.rs`. They take owned concrete inputs, return
//! `PropertyResult`, and never panic on the property side: panics from the
//! library-under-test are caught and surfaced as `Fail`.

use std::cell::RefCell;
use std::format;
use std::io::{Read, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;
use std::string::String;
use std::vec::Vec;

use crate::block::{
    compress_prepend_size_with_dict, decompress_size_prepended,
    decompress_size_prepended_with_dict,
};
use crate::frame::{FrameDecoder, FrameEncoder};

#[allow(unused_imports)]
use crate::block::DecompressError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropertyResult {
    Pass,
    Fail(String),
    Discard,
}

// ---------------------------------------------------------------------------
// Bug 1: AutoFinishEncoder Drop must mirror an explicit, ignored try_finish().
// (44e14b1 — Don't panic on drop)
// ---------------------------------------------------------------------------

/// A `Write` impl that always errors on `flush` and `write`. Used to drive
/// the underlying writer into the error path inside `Drop`.
struct AlwaysErrorWriter;

impl Write for AlwaysErrorWriter {
    fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
    }
}

/// A `Write` that appends every byte to a shared `Vec<u8>`. By using
/// `Rc<RefCell<Vec<u8>>>` we can keep observing the buffer after the encoder
/// (and therefore its writer) has been dropped, which is exactly what we
/// need to compare the two finish-then-drop paths byte-for-byte.
#[derive(Clone)]
struct SharedWriter {
    buf: Rc<RefCell<Vec<u8>>>,
}

impl Write for SharedWriter {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.buf.borrow_mut().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn shared() -> SharedWriter {
    SharedWriter { buf: Rc::new(RefCell::new(Vec::new())) }
}

/// Model-based check on `AutoFinishEncoder<W>::drop`.
///
/// The upstream docstring on `FrameEncoder::auto_finish()` explicitly states
/// "Errors on drop get silently ignored. If you want to handle errors then use
/// `finish()` or `try_finish()` instead." (src/frame/compress.rs:122-124,
/// 410-412). That fixes the contract: dropping an `AutoFinishEncoder<W>` is
/// observationally equivalent to calling `try_finish()` directly and
/// discarding the `Result` before letting the encoder fall out of scope.
///
/// The model below is exactly that "discard try_finish's Result, then drop"
/// pipeline. We compare it against `auto_finish()`-then-drop on three axes:
///
///   1. **Byte equivalence (working writer).** Encoding `data` through
///      `auto_finish()` and through the explicit `try_finish()`-and-discard
///      path must produce identical compressed byte streams.
///   2. **Roundtrip soundness (working writer).** The `auto_finish()` output
///      must decode back to `data` via `FrameDecoder`. (LZ4 frame is the
///      whole point of the encoder; a Drop that produces garbage would still
///      fail to roundtrip even if it didn't panic.)
///   3. **Totality (erroring writer).** With a writer whose `write`/`flush`
///      always return `BrokenPipe`, `auto_finish().drop()` must not panic —
///      the documented contract says drop "silently ignores" any error.
///
/// The buggy `Drop` impl `panic!("Error when flushing frame on drop ...")`
/// breaks axis (3) immediately. The model also catches a hypothetical buggy
/// drop that quietly truncates the stream (axes 1, 2), so the property is no
/// longer just "doesn't panic".
pub fn property_drop_matches_manual_finish(data: Vec<u8>) -> PropertyResult {
    // ----- Axis 1: byte equivalence on the auto_finish vs explicit path. -----
    let auto_writer = shared();
    let auto_buf_handle = auto_writer.buf.clone();
    let auto_path = catch_unwind(AssertUnwindSafe(|| {
        let mut auto = FrameEncoder::new(auto_writer).auto_finish();
        auto.write_all(&data)
            .map_err(|e| format!("auto_finish write_all errored: {e:?}"))?;
        // Drop happens at end of scope -> implicit try_finish.
        drop(auto);
        Ok::<(), String>(())
    }));
    if let Err(_) = auto_path {
        return PropertyResult::Fail(format!(
            "auto_finish path panicked with {} input bytes (drop must not panic on a working writer)",
            data.len()
        ));
    }
    if let Ok(Err(msg)) = auto_path {
        return PropertyResult::Fail(msg);
    }
    let auto_bytes: Vec<u8> = auto_buf_handle.borrow().clone();

    let manual_writer = shared();
    let manual_buf_handle = manual_writer.buf.clone();
    let manual_path = catch_unwind(AssertUnwindSafe(|| {
        let mut enc = FrameEncoder::new(manual_writer);
        enc.write_all(&data)
            .map_err(|e| format!("manual write_all errored: {e:?}"))?;
        // Model the documented Drop contract: try_finish() with Result discarded.
        let _ = enc.try_finish();
        drop(enc);
        Ok::<(), String>(())
    }));
    if let Err(_) = manual_path {
        return PropertyResult::Fail(
            "manual try_finish-and-drop model panicked (this should never happen on a Vec writer)"
                .to_string(),
        );
    }
    if let Ok(Err(msg)) = manual_path {
        return PropertyResult::Fail(msg);
    }
    let manual_bytes: Vec<u8> = manual_buf_handle.borrow().clone();

    if auto_bytes != manual_bytes {
        return PropertyResult::Fail(format!(
            "auto_finish drop produced {} bytes, manual try_finish+drop produced {} bytes (must be identical per documented Drop contract)",
            auto_bytes.len(),
            manual_bytes.len()
        ));
    }

    // ----- Axis 2: roundtrip soundness via FrameDecoder. -----
    let decoded = catch_unwind(AssertUnwindSafe(|| {
        let mut dec = FrameDecoder::new(&auto_bytes[..]);
        let mut out = Vec::new();
        dec.read_to_end(&mut out).map(|_| out)
    }));
    match decoded {
        Err(_) => {
            return PropertyResult::Fail(
                "FrameDecoder panicked decoding the auto_finish output".to_string(),
            )
        }
        Ok(Err(e)) => {
            return PropertyResult::Fail(format!(
                "FrameDecoder errored on auto_finish output: {e:?}"
            ))
        }
        Ok(Ok(out)) => {
            if out != data {
                return PropertyResult::Fail(format!(
                    "roundtrip mismatch: input.len()={}, decoded.len()={}",
                    data.len(),
                    out.len()
                ));
            }
        }
    }

    // ----- Axis 3: totality on an erroring writer. -----
    let erroring = catch_unwind(AssertUnwindSafe(|| {
        let mut auto = FrameEncoder::new(AlwaysErrorWriter).auto_finish();
        // Best-effort write; ignore the inevitable Err.
        let _ = auto.write_all(&data);
        drop(auto);
    }));
    if erroring.is_err() {
        return PropertyResult::Fail(format!(
            "AutoFinishEncoder drop panicked under an erroring writer with {} input bytes (the auto_finish docstring requires drop to silently ignore errors)",
            data.len()
        ));
    }

    PropertyResult::Pass
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

