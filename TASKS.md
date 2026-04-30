# lz4_flex — ETNA Tasks

Total tasks: 12

## Task Index

| Task | Variant | Framework | Property | Witness |
|------|---------|-----------|----------|---------|
| 001 | `decompress_short_input_no_panic_e0e7e5c_1` | proptest | `DecompressShortInputNoPanic` | `witness_decompress_short_input_no_panic_case_empty` |
| 002 | `decompress_short_input_no_panic_e0e7e5c_1` | quickcheck | `DecompressShortInputNoPanic` | `witness_decompress_short_input_no_panic_case_empty` |
| 003 | `decompress_short_input_no_panic_e0e7e5c_1` | crabcheck | `DecompressShortInputNoPanic` | `witness_decompress_short_input_no_panic_case_empty` |
| 004 | `decompress_short_input_no_panic_e0e7e5c_1` | hegel | `DecompressShortInputNoPanic` | `witness_decompress_short_input_no_panic_case_empty` |
| 005 | `dont_panic_on_drop_44e14b1_1` | proptest | `DontPanicOnDrop` | `witness_dont_panic_on_drop_case_small_payload` |
| 006 | `dont_panic_on_drop_44e14b1_1` | quickcheck | `DontPanicOnDrop` | `witness_dont_panic_on_drop_case_small_payload` |
| 007 | `dont_panic_on_drop_44e14b1_1` | crabcheck | `DontPanicOnDrop` | `witness_dont_panic_on_drop_case_small_payload` |
| 008 | `dont_panic_on_drop_44e14b1_1` | hegel | `DontPanicOnDrop` | `witness_dont_panic_on_drop_case_small_payload` |
| 009 | `small_dict_no_panic_2d83a3d_1` | proptest | `SmallDictNoPanic` | `witness_small_dict_no_panic_case_three_byte_dict` |
| 010 | `small_dict_no_panic_2d83a3d_1` | quickcheck | `SmallDictNoPanic` | `witness_small_dict_no_panic_case_three_byte_dict` |
| 011 | `small_dict_no_panic_2d83a3d_1` | crabcheck | `SmallDictNoPanic` | `witness_small_dict_no_panic_case_three_byte_dict` |
| 012 | `small_dict_no_panic_2d83a3d_1` | hegel | `SmallDictNoPanic` | `witness_small_dict_no_panic_case_three_byte_dict` |

## Witness Catalog

- `witness_decompress_short_input_no_panic_case_empty` — base passes, variant fails
- `witness_decompress_short_input_no_panic_case_one_byte` — base passes, variant fails
- `witness_decompress_short_input_no_panic_case_three_bytes` — base passes, variant fails
- `witness_dont_panic_on_drop_case_small_payload` — base passes, variant fails
- `witness_dont_panic_on_drop_case_empty_payload` — base passes, variant fails
- `witness_dont_panic_on_drop_case_repeated_bytes` — base passes, variant fails
- `witness_small_dict_no_panic_case_three_byte_dict` — base passes, variant fails
- `witness_small_dict_no_panic_case_two_byte_dict` — base passes, variant fails
- `witness_small_dict_no_panic_case_one_byte_dict` — base passes, variant fails
