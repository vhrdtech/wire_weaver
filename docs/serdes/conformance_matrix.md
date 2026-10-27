---
level: nomicon
---

# Conformance matrix

<!-- Generated from tests/conformance/corpus.json by `just conformance-update`, do not edit. -->

Every case of the [conformance corpus](conformance.md) and what each consumer does with it: ✅ encodes to the golden bytes and decodes them back to the value (or only decodes them, or rejects them, as the case's direction says); a gap names why the consumer can't run the case yet and the item that fixes it, *reader only* that it decodes but can't encode, *by design* that it behaves otherwise on purpose. The tests fail when a mark is missing or the consumer passes a marked case.

| Consumer | Cases run | Gaps |
|---|---|---|
| derive | 121 of 121 | 0 |
| dynamic | 109 of 121 | 12 |
| explain | 109 of 121 | 12 |
| python | 109 of 121 | 12 |
| c | 34 of 121 | 87 |
| derive | 131 of 131 | 0 |
| dynamic | 120 of 131 | 11 |
| explain | 120 of 131 | 11 |
| python | 120 of 131 | 11 |
| c | 34 of 131 | 97 |

| Case | Type | Direction | derive | dynamic | explain | python | c |
|---|---|---|---|---|---|---|---|
| `int.u8.zero` | `u8` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `int.u8.max` | `u8` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `int.u16.le` | `u16` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `int.u32.le` | `u32` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `int.u64.max` | `u64` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `int.u128.max` | `u128` | both | ✅ | ✅ | ✅ | ✅ | gap SW-66 |
| `int.i8.min` | `i8` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `int.i16.negative` | `i16` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `int.i32.negative` | `i32` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `int.i64.min` | `i64` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `int.i128.min` | `i128` | both | ✅ | ✅ | ✅ | ✅ | gap SW-66 |
| `float.f32` | `f32` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `float.f64` | `f64` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `bool.true` | `bool` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `bool.false` | `bool` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `unib32.one_nibble` | `unib32` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `unib32.two_nibbles` | `unib32` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `unib32.max` | `unib32` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `uvlq32.two_bytes` | `uvlq32` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `uvlq64.max` | `uvlq64` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `bits.nibble` | `nibble` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `bits.u4` | `u4` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `bits.u10` | `u10` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `bits.u20` | `u20` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `bits.i5` | `i5` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `bits.i12` | `i12` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `str.empty` | `string` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `str.ascii` | `string` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `str.utf8` | `string` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `str.array_string` | `string` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `bytes.empty` | `bytes` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `bytes.three` | `bytes` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `vec.u16` | `vec_u16` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `vec.array_vec` | `vec_u16` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `vec.string` | `vec_string` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `vec.bool` | `vec_bool` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `vec.option` | `vec_option` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `vec.vec` | `vec_vec` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `max_len.str.at_bound` | `max_len_str` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `max_len.str.over_bound` | `max_len_str` | reject value | ✅ | ✅ | ✅ | ✅ | ✅ |
| `max_len.str.over_bound_bytes` | `max_len_str` | reject | ✅ | ✅ | ✅ | ✅ | ✅ |
| `max_len.bytes` | `max_len_bytes` | both | ✅ | ✅ | ✅ | ✅ | ✅ |
| `option.none` | `option_u8` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `option.some` | `option_u8` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `option.str` | `option_str` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `option.option.some_some` | `option_option` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `option.option.some_none` | `option_option` | both | ✅ | gap TOOL-13 | gap TOOL-13 | gap TOOL-13 | gap API-26 |
| `result.ok` | `result` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `result.err` | `result` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `array.u16` | `array_u16` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `array.bool` | `array_bool` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `tuple` | `tuple` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `range` | `range` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `range_inclusive` | `range_inclusive` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `struct.bits` | `bits` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `struct.nested` | `nested` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `struct.flags.relocated` | `flagged` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `struct.flags.relocated_other` | `flagged` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `struct.tuple` | `pair` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `struct.tuple.none` | `pair` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `struct.tuple.plain` | `rgb` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `struct.unit` | `marker` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `struct.sized` | `sized` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `struct.final_structure` | `final_struct` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `struct.generic` | `generic` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `struct.phantom` | `phantom` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `struct.validate.ok` | `validated` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `struct.validate.rejected` | `validated` | reject | ✅ | gap SW-20 | gap SW-20 | gap SW-20 | gap API-26 |
| `struct.tail_size` | `Framed` | both | ✅ | gap INT-9 | gap INT-9 | gap INT-9 | gap API-26 |
| `struct.generic_const` | `generic_const` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `struct.sparse.empty` | `sparse` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `struct.sparse.slots` | `sparse` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `struct.recursive.list` | `tree` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `struct.recursive.box` | `expr` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `enum.unit` | `shape` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `enum.tuple` | `shape` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `enum.struct` | `shape` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `enum.explicit_discriminant` | `shape` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `enum.unknown_variant` | `shape` | decode | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `enum.appended_variant` | `shape_v2` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `enum.sub_byte.seven` | `address` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `enum.sub_byte.ten` | `address` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `enum.sized` | `command` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `enum.sized.wide_discriminant` | `command` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `enum.u2` | `error` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `enum.nibble.unit` | `nib_enum` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `enum.nibble.payload` | `nib_enum` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `enum.cfg.kept` | `cfg_subset` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `enum.cfg.kept_after_gap` | `cfg_subset` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `enum.cfg.compiled_out` | `cfg_subset` | reject | ✅ | by design (DER-8) | by design (DER-8) | by design (DER-8) | gap API-26 |
| `evolution.v1` | `config_v1` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `evolution.v2` | `config_v2` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `evolution.new_reads_old` | `config_v2` | decode | ✅ | gap INT-10 | gap INT-10 | gap INT-10 | gap API-26 |
| `evolution.old_reads_new` | `config_v1` | decode | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `fixed.numbers` | `fixed_numbers` | both | ✅ | ✅ | ✅ | ✅ | reader only (API-26) |
| `fixed.bits` | `fixed_bits` | both | ✅ | ✅ | ✅ | ✅ | reader only (API-26) |
| `fixed.nested` | `fixed_nested` | both | ✅ | ✅ | ✅ | ✅ | reader only (API-26) |
| `fixed.option.none` | `fixed_maybe` | both | ✅ | ✅ | ✅ | ✅ | reader only (API-26) |
| `fixed.option.some` | `fixed_maybe` | both | ✅ | ✅ | ✅ | ✅ | reader only (API-26) |
| `fixed.enum.unit` | `fixed_op` | both | ✅ | ✅ | ✅ | ✅ | reader only (API-26) |
| `fixed.enum.fields` | `fixed_op` | both | ✅ | ✅ | ✅ | ✅ | reader only (API-26) |
| `series.u16` | `series_u16` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `series.struct` | `series_sample` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `wire.struct` | `wired` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `wire.struct.out_of_range` | `wired` | reject value | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `wire.enum.tuple` | `wired_op` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `wire.enum.named` | `wired_op` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `wire.series` | `series_wired` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `cfg_attr.v1` | `gated_v1` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `cfg_attr.v2` | `gated_v2` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `cfg_attr.new_reads_old` | `gated_v2` | decode | ✅ | gap INT-10 | gap INT-10 | gap INT-10 | gap API-26 |
| `transparent.u64` | `count` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `transparent.unib32` | `index` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `transparent.fields.plain` | `plain_fields` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `transparent.fields` | `wrapped_fields` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `transparent.sized.plain` | `plain_sized` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `transparent.sized` | `wrapped_sized` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `transparent.vec.unib32.plain` | `vec_unib32` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `transparent.vec.unib32` | `vec_index` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `transparent.vec.nibble.plain` | `vec_nibble` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `transparent.vec.nibble` | `vec_digit` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `transparent.option.plain` | `option_u64` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `transparent.option` | `option_count` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `transparent.array.plain` | `array_nibble` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `transparent.array` | `array_digit` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `transparent.series.plain` | `series_u64` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `transparent.series` | `series_count` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `transparent.series.struct.plain` | `series_plain` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `transparent.series.struct` | `series_wrapped` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `head_tail.struct` | `HtRecord` | both | ✅ | gap SW-46 | gap SW-46 | gap SW-46 | reader only (API-26) |
| `link_setup.without_meta` | `LinkSetup` | both | ✅ | gap - | gap - | gap - | gap - |
| `link_setup.with_meta` | `LinkSetup` | both | ✅ | gap - | gap - | gap - | gap - |
| `disconnect_reason.hub_handover` | `DisconnectReason` | both | ✅ | gap - | gap - | gap - | gap - |
| `disconnect_reason.from_newer` | `DisconnectReason` | both | ✅ | gap - | gap - | gap - | gap - |
| `tail_bytes` | `WithTail` | both | ✅ | gap INT-9 | gap INT-9 | gap INT-9 | gap API-26 |
| `stdlib.timebase.steer_point` | `steer_point` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.timebase.steer_point.steerer` | `steer_point` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.timebase.sync_status` | `sync_status` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.timebase.sync_status.steerers` | `sync_status` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |

## Gaps

- **c**: no generated C for this type yet: C from the ww_self model (API-26). 73 cases: `vec.u16`, `vec.array_vec`, `vec.string`, `vec.bool`, `vec.option`, `vec.vec`, `option.none`, `option.some`, `option.str`, `option.option.some_some`, `option.option.some_none`, `result.ok`, `result.err`, `array.u16`, `array.bool`, `tuple`, `range`, `range_inclusive`, `struct.bits`, `struct.nested`, `struct.flags.relocated`, `struct.flags.relocated_other`, `struct.tuple`, `struct.tuple.none`, `struct.tuple.plain`, `struct.unit`, `struct.sized`, `struct.final_structure`, `struct.generic`, `struct.phantom`, `struct.validate.ok`, `struct.validate.rejected`, `struct.tail_size`, `struct.generic_const`, `struct.sparse.empty`, `struct.sparse.slots`, `struct.recursive.list`, `struct.recursive.box`, `enum.unit`, `enum.tuple`, `enum.struct`, `enum.explicit_discriminant`, `enum.unknown_variant`, `enum.appended_variant`, `enum.sub_byte.seven`, `enum.sub_byte.ten`, `enum.sized`, `enum.sized.wide_discriminant`, `enum.u2`, `enum.nibble.unit`, `enum.nibble.payload`, `enum.cfg.kept`, `enum.cfg.kept_after_gap`, `enum.cfg.compiled_out`, `evolution.v1`, `evolution.v2`, `evolution.new_reads_old`, `evolution.old_reads_new`, `series.u16`, `series.struct`, `wire.struct`, `wire.struct.out_of_range`, `wire.enum.tuple`, `wire.enum.named`, `wire.series`, `cfg_attr.v1`, `cfg_attr.v2`, `cfg_attr.new_reads_old`, `tail_bytes`, `stdlib.timebase.steer_point`, `stdlib.timebase.steer_point.steerer`, `stdlib.timebase.sync_status`, `stdlib.timebase.sync_status.steerers`.
- **c**: no generated C for this type yet: C from the ww_self model (API-26). 83 cases: `vec.u16`, `vec.array_vec`, `vec.string`, `vec.bool`, `vec.option`, `vec.vec`, `option.none`, `option.some`, `option.str`, `option.option.some_some`, `option.option.some_none`, `result.ok`, `result.err`, `array.u16`, `array.bool`, `tuple`, `range`, `range_inclusive`, `struct.bits`, `struct.nested`, `struct.flags.relocated`, `struct.flags.relocated_other`, `struct.tuple`, `struct.tuple.none`, `struct.tuple.plain`, `struct.unit`, `struct.sized`, `struct.final_structure`, `struct.generic`, `struct.phantom`, `struct.validate.ok`, `struct.validate.rejected`, `struct.tail_size`, `struct.generic_const`, `struct.sparse.empty`, `struct.sparse.slots`, `struct.recursive.list`, `struct.recursive.box`, `enum.unit`, `enum.tuple`, `enum.struct`, `enum.explicit_discriminant`, `enum.unknown_variant`, `enum.appended_variant`, `enum.sub_byte.seven`, `enum.sub_byte.ten`, `enum.sized`, `enum.sized.wide_discriminant`, `enum.u2`, `enum.nibble.unit`, `enum.nibble.payload`, `enum.cfg.kept`, `enum.cfg.kept_after_gap`, `enum.cfg.compiled_out`, `evolution.v1`, `evolution.v2`, `evolution.new_reads_old`, `evolution.old_reads_new`, `series.u16`, `series.struct`, `transparent.u64`, `transparent.unib32`, `transparent.fields.plain`, `transparent.fields`, `transparent.sized.plain`, `transparent.sized`, `transparent.vec.unib32.plain`, `transparent.vec.unib32`, `transparent.vec.nibble.plain`, `transparent.vec.nibble`, `transparent.option.plain`, `transparent.option`, `transparent.array.plain`, `transparent.array`, `transparent.series.plain`, `transparent.series`, `transparent.series.struct.plain`, `transparent.series.struct`, `tail_bytes`, `stdlib.timebase.steer_point`, `stdlib.timebase.steer_point.steerer`, `stdlib.timebase.sync_status`, `stdlib.timebase.sync_status.steerers`.
- **c**: reader only: the generated C has no writer yet (API-26). 8 cases: `fixed.numbers`, `fixed.bits`, `fixed.nested`, `fixed.option.none`, `fixed.option.some`, `fixed.enum.unit`, `fixed.enum.fields`, `head_tail.struct`.
- **c**: the C runtime has no 128-bit numbers (SW-66). 2 cases: `int.u128.max`, `int.i128.min`.
- **c**: ww_link's Disconnect payload, not a type of an API in the model. 2 cases: `disconnect_reason.hub_handover`, `disconnect_reason.from_newer`.
- **c**: ww_link's LinkSetup is a link message, not a type of an API in the model. 2 cases: `link_setup.without_meta`, `link_setup.with_meta`.
- **dynamic**: TailBytes has no form in the API model yet (INT-9). 1 case: `tail_bytes`.
- **dynamic**: TailSize has no form in the API model yet (INT-9). 1 case: `struct.tail_size`.
- **dynamic**: a validate function is Rust code the API model can't carry; checks in the model come with sub-type numbers (SW-20). 1 case: `struct.validate.rejected`.
- **dynamic**: by design: the API model has the variants of every build, a host reads it (DER-8). 1 case: `enum.cfg.compiled_out`.
- **dynamic**: head_tail is a wire format of its own without ShrinkWrap serdes, not an API type (SW-46). 1 case: `head_tail.struct`.
- **dynamic**: null is None and Some(None) alike in the corpus value form, as in Python (TOOL-13). 1 case: `option.option.some_none`.
- **dynamic**: the API model has no #[default] values, an old field reads as its type's default (INT-10). 2 cases: `evolution.new_reads_old`, `cfg_attr.new_reads_old`.
- **dynamic**: ww_link's Disconnect payload, not a type of an API in the model; a dynamic consumer reads API types (SW-46 is the same for head_tail). 2 cases: `disconnect_reason.hub_handover`, `disconnect_reason.from_newer`.
- **dynamic**: ww_link's LinkSetup is a link message, not a type of an API in the model; a dynamic consumer reads API types (SW-46 is the same for head_tail). 2 cases: `link_setup.without_meta`, `link_setup.with_meta`.
- **explain**: TailBytes has no form in the API model yet (INT-9). 1 case: `tail_bytes`.
- **explain**: TailSize has no form in the API model yet (INT-9). 1 case: `struct.tail_size`.
- **explain**: a validate function is Rust code the API model can't carry; checks in the model come with sub-type numbers (SW-20). 1 case: `struct.validate.rejected`.
- **explain**: by design: the API model has the variants of every build, a host reads it (DER-8). 1 case: `enum.cfg.compiled_out`.
- **explain**: head_tail is a wire format of its own without ShrinkWrap serdes, not an API type (SW-46). 1 case: `head_tail.struct`.
- **explain**: null is None and Some(None) alike in the corpus value form, as in Python (TOOL-13). 1 case: `option.option.some_none`.
- **explain**: the API model has no #[default] values, an old field reads as its type's default (INT-10). 2 cases: `evolution.new_reads_old`, `cfg_attr.new_reads_old`.
- **explain**: ww_link's Disconnect payload, not a type of an API in the model. 2 cases: `disconnect_reason.hub_handover`, `disconnect_reason.from_newer`.
- **explain**: ww_link's LinkSetup is a link message, not a type of an API in the model. 2 cases: `link_setup.without_meta`, `link_setup.with_meta`.
- **python**: TailBytes has no form in the API model yet (INT-9). 1 case: `tail_bytes`.
- **python**: TailSize has no form in the API model yet (INT-9). 1 case: `struct.tail_size`.
- **python**: a validate function is Rust code the API model can't carry; checks in the model come with sub-type numbers (SW-20). 1 case: `struct.validate.rejected`.
- **python**: by design: the API model has the variants of every build (DER-8). 1 case: `enum.cfg.compiled_out`.
- **python**: head_tail is a wire format of its own without ShrinkWrap serdes, not an API type (SW-46). 1 case: `head_tail.struct`.
- **python**: null is None and Some(None) alike in the Python value form (TOOL-13). 1 case: `option.option.some_none`.
- **python**: the API model has no #[default] values (INT-10). 2 cases: `evolution.new_reads_old`, `cfg_attr.new_reads_old`.
- **python**: ww_link's Disconnect payload, not a type of an API in the model. 2 cases: `disconnect_reason.hub_handover`, `disconnect_reason.from_newer`.
- **python**: ww_link's LinkSetup is a link message, not a type of an API in the model. 2 cases: `link_setup.without_meta`, `link_setup.with_meta`.
