# lz4_flex — Injected Bugs

Pure Rust LZ4 block + frame compression — ETNA workload.

Total mutations: 3

## Bug Index

| # | Variant | Name | Location | Injection | Fix Commit |
|---|---------|------|----------|-----------|------------|
| 1 | `decompress_short_input_no_panic_e0e7e5c_1` | `decompress_short_input_no_panic` | `src/block/decompress.rs:496` | `patch` | `e0e7e5c223441c7739c9a140c1e62db0399621ba` |
| 2 | `dont_panic_on_drop_44e14b1_1` | `dont_panic_on_drop` | `src/frame/compress.rs:425` | `marauders` | `44e14b15e51daaf010a554be07ee60ea95522c8d` |
| 3 | `small_dict_no_panic_2d83a3d_1` | `small_dict_no_panic` | `src/block/compress.rs:626` | `patch` | `2d83a3da281266aeb2928272001043ecc04a8fe4` |

## Property Mapping

| Variant | Property | Witness(es) |
|---------|----------|-------------|
| `decompress_short_input_no_panic_e0e7e5c_1` | `DecompressShortInputNoPanic` | `witness_decompress_short_input_no_panic_case_empty`, `witness_decompress_short_input_no_panic_case_one_byte`, `witness_decompress_short_input_no_panic_case_three_bytes` |
| `dont_panic_on_drop_44e14b1_1` | `DontPanicOnDrop` | `witness_dont_panic_on_drop_case_small_payload`, `witness_dont_panic_on_drop_case_empty_payload`, `witness_dont_panic_on_drop_case_repeated_bytes` |
| `small_dict_no_panic_2d83a3d_1` | `SmallDictNoPanic` | `witness_small_dict_no_panic_case_three_byte_dict`, `witness_small_dict_no_panic_case_two_byte_dict`, `witness_small_dict_no_panic_case_one_byte_dict` |

## Framework Coverage

| Property | proptest | quickcheck | crabcheck | hegel |
|----------|---------:|-----------:|----------:|------:|
| `DecompressShortInputNoPanic` | ✓ | ✓ | ✓ | ✓ |
| `DontPanicOnDrop` | ✓ | ✓ | ✓ | ✓ |
| `SmallDictNoPanic` | ✓ | ✓ | ✓ | ✓ |

## Bug Details

### 1. decompress_short_input_no_panic

- **Variant**: `decompress_short_input_no_panic_e0e7e5c_1`
- **Location**: `src/block/decompress.rs:496` (inside `decompress_size_prepended`)
- **Property**: `DecompressShortInputNoPanic`
- **Witness(es)**:
  - `witness_decompress_short_input_no_panic_case_empty`
  - `witness_decompress_short_input_no_panic_case_one_byte`
  - `witness_decompress_short_input_no_panic_case_three_bytes`
- **Source**: [#8](https://github.com/PSeitz/lz4_flex/pull/8) — Don't panic passing an empty buffer to decompress_size_prepended
  > `decompress_size_prepended` directly indexed `input[0..=3]` to recover the prepended uncompressed size. Calling it with an input shorter than 4 bytes (in particular the empty slice) panicked with 'index out of bounds: the len is 0 but the index is 0' before any decompression code ran. The fix routes the read through a `super::uncompressed_size` helper that returns `Err(ExpectedAnotherByte)` when the input is too short.
- **Fix commit**: `e0e7e5c223441c7739c9a140c1e62db0399621ba` — Don't panic passing an empty buffer to decompress_size_prepended
- **Invariant violated**: `decompress_size_prepended(input)` must return `Err(_)` (never panic) when `input.len() < 4`. The first 4 bytes are the prepended little-endian uncompressed length; a buffer shorter than that cannot encode a valid frame and must be reported as `ExpectedAnotherByte`.
- **How the mutation triggers**: The buggy code reads `input[0]`, `input[1]`, `input[2]`, `input[3]` directly to decode the size, then slices `&input[4..]` for the body. With `input.len() < 4` the very first index out-of-bounds panics; with `input.len() == 4` only the slice survives but trivial cases like the empty input always abort.

### 2. dont_panic_on_drop

- **Variant**: `dont_panic_on_drop_44e14b1_1`
- **Location**: `src/frame/compress.rs:425` (inside `<AutoFinishEncoder as Drop>::drop`)
- **Property**: `DontPanicOnDrop`
- **Witness(es)**:
  - `witness_dont_panic_on_drop_case_small_payload`
  - `witness_dont_panic_on_drop_case_empty_payload`
  - `witness_dont_panic_on_drop_case_repeated_bytes`
- **Source**: [#98](https://github.com/PSeitz/lz4_flex/pull/98) — Don't panic on drop
  > `AutoFinishEncoder::drop` called `panic!` when its implicit `try_finish()` returned an error from the underlying writer. Dropping the encoder while a writer was failing therefore aborted the program — an unrecoverable surprise for callers who chose `auto_finish` precisely so they wouldn't have to handle finish errors. The fix swallows the result with `let _ = encoder.try_finish();`.
- **Fix commit**: `44e14b15e51daaf010a554be07ee60ea95522c8d` — Don't panic on drop
- **Invariant violated**: `AutoFinishEncoder<W>` dropped at end of scope must never panic, regardless of whether the underlying writer's `flush`/`write` returned an error during the implicit `try_finish()`.
- **How the mutation triggers**: The buggy `Drop` impl pattern-matches `try_finish()`'s `Err(err)` arm and calls `panic!("Error when flushing frame on drop {err:?}")`. With a writer that returns `Err(io::ErrorKind::BrokenPipe)` the buggy drop unconditionally aborts the test thread; the fix discards the result so drop becomes total.

### 3. small_dict_no_panic

- **Variant**: `small_dict_no_panic_2d83a3d_1`
- **Location**: `src/block/compress.rs:626` (inside `compress_into_vec_with_dict`)
- **Property**: `SmallDictNoPanic`
- **Witness(es)**:
  - `witness_small_dict_no_panic_case_three_byte_dict`
  - `witness_small_dict_no_panic_case_two_byte_dict`
  - `witness_small_dict_no_panic_case_one_byte_dict`
- **Source**: [#131](https://github.com/PSeitz/lz4_flex/issues/131) — Fix: Small dict leads to panic
  > `compress_into_vec_with_dict` forwarded any caller-supplied dictionary into `compress_internal` even when its length was less than `MINMATCH` (4 bytes). The hot loop then sliced past the end of the dict (`get_hash_at` reads `size_of::<usize>()` bytes), panicking with 'range end index … out of range for slice of length …'. The fix special-cases dicts shorter than 4 bytes and replaces them with an empty slice.
- **Fix commit**: `2d83a3da281266aeb2928272001043ecc04a8fe4` — Fix: Small dict leads to panic
- **Invariant violated**: `compress_prepend_size_with_dict(input, dict)` followed by `decompress_size_prepended_with_dict(.., dict)` must roundtrip `input` exactly and must not panic, regardless of `dict.len()` — including the boundary `dict.len() < 4`.
- **How the mutation triggers**: The buggy implementation drops the `mut dict_data: &[u8]` and the `if dict_data.len() <= 3 { dict_data = b""; }` guard. `compress_internal` then enters the hot match-search loop with a dict shorter than `size_of::<usize>()` bytes, panicking inside `get_hash_at`'s slice index expression on the very first probe.

## Dropped Candidates

- `b078f7c` (Fix: Out of bounds write) — OOB write only observable under MemorySanitizer/AddressSanitizer; the over-allocated output Vec absorbs the buggy write under default cargo test, so no panic, no detectable behavior change without sanitizer infrastructure.
- `055502e` (fix handling of invalid match offsets during decompression) — Buggy safe-decode path silently produces wrong output (offset clamping + uninitialised-byte exposure) instead of erroring. Detecting requires either a known-good reference decoder for byte-for-byte comparison or MemorySanitizer; both are out of reach for the per-variant witness contract here.
- `2991a09` (fix get_maximum_output_size overflow on 32-bit targets) — Bug only triggers on 32-bit targets (input_len * 110 overflows usize when usize is 32-bit). x86_64 host of the etna workload cannot reproduce it.
- `c1483c4` (fix the issue (read_integer u32 -> usize)) — The buggy u32 accumulator only overflows when a single LZ4 literal/match length encoding spans more than 2^32 bytes, which the upstream block format caps below 64KB per block — unreachable in practice.
