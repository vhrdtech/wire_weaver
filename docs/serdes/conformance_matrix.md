---
level: nomicon
---

# Conformance matrix

<!-- Generated from tests/conformance/corpus.json by `just conformance-update`, do not edit. -->

Every case of the [conformance corpus](conformance.md) and what each consumer does with it: ✅ encodes to the golden bytes and decodes them back to the value (or only decodes them, or rejects them, as the case's direction says); a gap names why the consumer can't run the case yet and the item that fixes it, *reader only* that it decodes but can't encode, *by design* that it behaves otherwise on purpose. The tests fail when a mark is missing or the consumer passes a marked case.

| Consumer | Cases run | Gaps |
|---|---|---|
| derive | 184 of 184 | 0 |
| dynamic | 168 of 184 | 16 |
| explain | 168 of 184 | 16 |
| python | 168 of 184 | 16 |
| c | 34 of 184 | 150 |
| derive | 183 of 183 | 0 |
| dynamic | 163 of 183 | 20 |
| explain | 163 of 183 | 20 |
| python | 163 of 183 | 20 |
| c | 34 of 183 | 149 |

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
| `wire.struct` | `wired` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `wire.struct.out_of_range` | `wired` | reject value | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `wire.enum.tuple` | `wired_op` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `wire.enum.named` | `wired_op` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `wire.series` | `series_wired` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `cfg_attr.v1` | `gated_v1` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `cfg_attr.v2` | `gated_v2` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `cfg_attr.new_reads_old` | `gated_v2` | decode | ✅ | gap INT-10 | gap INT-10 | gap INT-10 | gap API-26 |
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
| `stdlib.si.dim.volt` | `dim` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.dim.rad_bit` | `dim` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.dim.counted` | `dim` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.dim.rejected` | `dim` | reject | ✅ | gap SW-20 | gap SW-20 | gap SW-20 | gap API-26 |
| `stdlib.si.dim.root` | `dim` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.dim.root.rejected` | `dim` | reject | ✅ | gap SW-20 | gap SW-20 | gap SW-20 | gap API-26 |
| `stdlib.si.fixed.pow10` | `fixed_pow10` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.fixed.duty16` | `duty16` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.fixed.bounded` | `fixed_bounded` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.fixed.bounded.rejected` | `fixed_bounded` | reject | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.fixed.customary` | `fixed_customary` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.quantity.milli` | `quantity_volt` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.quantity.kibi` | `quantity_byte` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.quantity.pow10` | `quantity_second` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.quantity.pow2` | `quantity_second` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.quantity.fullscale` | `quantity_ratio` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.quantity.bounded` | `quantity_rail` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.quantity.bounded.rejected` | `quantity_rail` | reject | ✅ | gap CLT-54 | gap CLT-54 | gap CLT-54 | gap API-26 |
| `stdlib.si.magnitude.one` | `magnitude` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.magnitude.milli` | `magnitude` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.magnitude.kibi` | `magnitude` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.magnitude.duty16` | `magnitude` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.magnitude.degree` | `magnitude` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.magnitude.sqrt2` | `magnitude` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.magnitude.decibel_in_neper` | `magnitude` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.magnitude.pound` | `magnitude` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.magnitude.rejected` | `magnitude` | reject | ✅ | gap SW-20 | gap SW-20 | gap SW-20 | gap API-26 |
| `stdlib.si.point.celsius` | `point_celsius` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.point.bounded` | `point_bounded` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.point.bounded.rejected` | `point_bounded` | reject | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.index.in_struct` | `with_index` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.index_from_1` | `index_from_1` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.index_from_1.zero.rejected` | `index_from_1` | reject | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.index_from_1.bounded.rejected` | `index_from_1` | reject | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.seq.nibble_wrap` | `seq_pair` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.offset.zero` | `offset` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.offset.celsius` | `offset` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.offset.minus_forty` | `offset` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.offset.fahrenheit` | `offset` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.offset.rejected` | `offset` | reject | ✅ | gap SW-20 | gap SW-20 | gap SW-20 | gap API-26 |
| `stdlib.si.si_expr.volt` | `si_expr` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.si_expr.pound_force` | `si_expr` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.si_expr.sqrt` | `si_expr` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.si_expr.rejected` | `si_expr` | reject | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.marker_def.unit` | `marker_def` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.marker_def.unit_conventional` | `marker_def` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.marker_def.scale` | `marker_def` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.marker_def.per` | `marker_def` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.marker_def.origin` | `marker_def` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.marker_def.angle` | `marker_def` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.item_struct_extra.transparent` | `item_struct_extra` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.item_struct_extra.marker` | `item_struct_extra` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |

## Requantize

A stored number converted to another scale and storage with a named rounding and overflow policy (STD-63, [Rounding and overflow](../std_library/rounding.md)): every consumer gives the result, `none` for no value.

| Case | Conversion | Value | Rounding | Overflow | Result | derive | dynamic | python | c |
|---|---|---|---|---|---|---|---|---|---|
| `requantize.milli_to_deci.tie.truncate` | mV i16 -> dV i8 | 1250 | truncate | checked | 12 | ✅ | ✅ | ✅ | ✅ |
| `requantize.milli_to_deci.tie.floor` | mV i16 -> dV i8 | 1250 | floor | checked | 12 | ✅ | ✅ | ✅ | ✅ |
| `requantize.milli_to_deci.tie.ceil` | mV i16 -> dV i8 | 1250 | ceil | checked | 13 | ✅ | ✅ | ✅ | ✅ |
| `requantize.milli_to_deci.tie.nearest_even` | mV i16 -> dV i8 | 1250 | nearest_even | checked | 12 | ✅ | ✅ | ✅ | ✅ |
| `requantize.milli_to_deci.tie.nearest_away` | mV i16 -> dV i8 | 1250 | nearest_away | checked | 13 | ✅ | ✅ | ✅ | ✅ |
| `requantize.milli_to_deci.negative_tie.truncate` | mV i16 -> dV i8 | -1250 | truncate | checked | -12 | ✅ | ✅ | ✅ | ✅ |
| `requantize.milli_to_deci.negative_tie.floor` | mV i16 -> dV i8 | -1250 | floor | checked | -13 | ✅ | ✅ | ✅ | ✅ |
| `requantize.milli_to_deci.negative_tie.ceil` | mV i16 -> dV i8 | -1250 | ceil | checked | -12 | ✅ | ✅ | ✅ | ✅ |
| `requantize.milli_to_deci.negative_tie.nearest_even` | mV i16 -> dV i8 | -1250 | nearest_even | checked | -12 | ✅ | ✅ | ✅ | ✅ |
| `requantize.milli_to_deci.negative_tie.nearest_away` | mV i16 -> dV i8 | -1250 | nearest_away | checked | -13 | ✅ | ✅ | ✅ | ✅ |
| `requantize.milli_to_deci.tie_up_to_even` | mV i16 -> dV i8 | 1350 | nearest_even | checked | 14 | ✅ | ✅ | ✅ | ✅ |
| `requantize.milli_to_deci.below_tie` | mV i16 -> dV i8 | 1249 | nearest_even | checked | 12 | ✅ | ✅ | ✅ | ✅ |
| `requantize.milli_to_deci.overflow.checked` | mV i16 -> dV i8 | 20000 | nearest_even | checked | none | ✅ | ✅ | ✅ | ✅ |
| `requantize.milli_to_deci.overflow.saturate` | mV i16 -> dV i8 | 20000 | nearest_even | saturate | 127 | ✅ | ✅ | ✅ | ✅ |
| `requantize.milli_to_deci.overflow.wrap` | mV i16 -> dV i8 | 20000 | nearest_even | wrap | -56 | ✅ | ✅ | ✅ | ✅ |
| `requantize.milli_to_deci.underflow.saturate` | mV i16 -> dV i8 | -20000 | nearest_even | saturate | -128 | ✅ | ✅ | ✅ | ✅ |
| `requantize.micro_to_milli_unsigned.negative.checked` | uV i32 -> mV u8 | -1500 | floor | checked | none | ✅ | ✅ | ✅ | ✅ |
| `requantize.micro_to_milli_unsigned.negative.saturate` | uV i32 -> mV u8 | -1500 | floor | saturate | 0 | ✅ | ✅ | ✅ | ✅ |
| `requantize.micro_to_milli_unsigned.negative.wrap` | uV i32 -> mV u8 | -1500 | floor | wrap | 254 | ✅ | ✅ | ✅ | ✅ |
| `requantize.micro_to_milli_unsigned.top` | uV i32 -> mV u8 | 255499 | nearest_even | checked | 255 | ✅ | ✅ | ✅ | ✅ |
| `requantize.micro_to_milli_unsigned.tie_past_top.checked` | uV i32 -> mV u8 | 255500 | nearest_even | checked | none | ✅ | ✅ | ✅ | ✅ |
| `requantize.micro_to_milli_unsigned.tie_past_top.saturate` | uV i32 -> mV u8 | 255500 | nearest_even | saturate | 255 | ✅ | ✅ | ✅ | ✅ |
| `requantize.q15_to_q7.tie_past_top.checked` | Q15 -> Q7 | 16448 | nearest_even | checked | none | ✅ | ✅ | ✅ | ✅ |
| `requantize.q15_to_q7.tie_past_top.saturate` | Q15 -> Q7 | 16448 | nearest_even | saturate | 127 | ✅ | ✅ | ✅ | ✅ |
| `requantize.q15_to_q7.tie.nearest_even` | Q15 -> Q7 | 16192 | nearest_even | checked | 126 | ✅ | ✅ | ✅ | ✅ |
| `requantize.q15_to_q7.tie.nearest_away` | Q15 -> Q7 | 16192 | nearest_away | checked | 127 | ✅ | ✅ | ✅ | ✅ |
| `requantize.q15_to_q7.negative_end` | Q15 -> Q7 | -16448 | nearest_even | checked | -128 | ✅ | ✅ | ✅ | ✅ |
| `requantize.seven_thirds.truncate` | 7/3 i16 -> i16 | 100 | truncate | checked | 233 | ✅ | ✅ | ✅ | ✅ |
| `requantize.seven_thirds.ceil` | 7/3 i16 -> i16 | 100 | ceil | checked | 234 | ✅ | ✅ | ✅ | ✅ |
| `requantize.seven_thirds.nearest_even` | 7/3 i16 -> i16 | 100 | nearest_even | checked | 233 | ✅ | ✅ | ✅ | ✅ |
| `requantize.seven_thirds.negative.floor` | 7/3 i16 -> i16 | -100 | floor | checked | -234 | ✅ | ✅ | ✅ | ✅ |
| `requantize.seven_thirds.negative.truncate` | 7/3 i16 -> i16 | -100 | truncate | checked | -233 | ✅ | ✅ | ✅ | ✅ |
| `requantize.seven_thirds.exact_top` | 7/3 i16 -> i16 | 14043 | truncate | checked | 32767 | ✅ | ✅ | ✅ | ✅ |
| `requantize.seven_thirds.past_top.checked` | 7/3 i16 -> i16 | 14044 | nearest_even | checked | none | ✅ | ✅ | ✅ | ✅ |
| `requantize.seven_thirds.past_top.saturate` | 7/3 i16 -> i16 | 14044 | nearest_even | saturate | 32767 | ✅ | ✅ | ✅ | ✅ |
| `requantize.seven_thirds.past_top.wrap` | 7/3 i16 -> i16 | 14044 | nearest_even | wrap | -32767 | ✅ | ✅ | ✅ | ✅ |
| `requantize.milli_to_micro.fits` | mV u16 -> uV u16 | 65 | truncate | checked | 65000 | ✅ | ✅ | ✅ | ✅ |
| `requantize.milli_to_micro.overflow.checked` | mV u16 -> uV u16 | 70 | truncate | checked | none | ✅ | ✅ | ✅ | ✅ |
| `requantize.milli_to_micro.overflow.saturate` | mV u16 -> uV u16 | 70 | truncate | saturate | 65535 | ✅ | ✅ | ✅ | ✅ |
| `requantize.milli_to_micro.overflow.wrap` | mV u16 -> uV u16 | 70 | truncate | wrap | 4464 | ✅ | ✅ | ✅ | ✅ |
| `requantize.bam16_to_bam8.truncate` | BAM16 -> BAM8 | 65408 | truncate | checked | 255 | ✅ | ✅ | ✅ | ✅ |
| `requantize.bam16_to_bam8.full_turn.checked` | BAM16 -> BAM8 | 65408 | nearest_even | checked | none | ✅ | ✅ | ✅ | ✅ |
| `requantize.bam16_to_bam8.full_turn.saturate` | BAM16 -> BAM8 | 65408 | nearest_even | saturate | 255 | ✅ | ✅ | ✅ | ✅ |
| `requantize.bam16_to_bam8.full_turn.wrap` | BAM16 -> BAM8 | 65408 | nearest_even | wrap | 0 | ✅ | ✅ | ✅ | ✅ |
| `requantize.bam16_to_bam8.tie` | BAM16 -> BAM8 | 16512 | nearest_even | checked | 64 | ✅ | ✅ | ✅ | ✅ |
| `requantize.float_to_int.tie.nearest_even` | f32 -> i16 | 2.5 | nearest_even | checked | 2 | ✅ | ✅ | ✅ | ✅ |
| `requantize.float_to_int.tie.nearest_away` | f32 -> i16 | 2.5 | nearest_away | checked | 3 | ✅ | ✅ | ✅ | ✅ |
| `requantize.float_to_int.negative_tie.nearest_even` | f32 -> i16 | -2.5 | nearest_even | checked | -2 | ✅ | ✅ | ✅ | ✅ |
| `requantize.float_to_int.negative.floor` | f32 -> i16 | -2.5 | floor | checked | -3 | ✅ | ✅ | ✅ | ✅ |
| `requantize.float_to_int.tie_past_top.checked` | f32 -> i16 | 32767.5 | nearest_even | checked | none | ✅ | ✅ | ✅ | ✅ |
| `requantize.float_to_int.tie_past_top.saturate` | f32 -> i16 | 32767.5 | nearest_even | saturate | 32767 | ✅ | ✅ | ✅ | ✅ |
| `requantize.float_to_int.huge.saturate` | f32 -> i16 | 10000000000.0 | truncate | saturate | 32767 | ✅ | ✅ | ✅ | ✅ |
| `requantize.float_to_int.huge.wrap` | f32 -> i16 | -10000000000.0 | truncate | wrap | 7168 | ✅ | ✅ | ✅ | ✅ |
| `requantize.int_to_float.milli` | mV i32 -> V f64 | 1250 | nearest_even | checked | 1.25 | ✅ | ✅ | ✅ | ✅ |
| `requantize.degree_to_radian.45000.truncate` | m° i32 -> urad i32 | 45000 | truncate | checked | 785398 | ✅ | ✅ | ✅ | ✅ |
| `requantize.degree_to_radian.45000.floor` | m° i32 -> urad i32 | 45000 | floor | checked | 785398 | ✅ | ✅ | ✅ | ✅ |
| `requantize.degree_to_radian.45000.ceil` | m° i32 -> urad i32 | 45000 | ceil | checked | 785399 | ✅ | ✅ | ✅ | ✅ |
| `requantize.degree_to_radian.45000.nearest_even` | m° i32 -> urad i32 | 45000 | nearest_even | checked | 785398 | ✅ | ✅ | ✅ | ✅ |
| `requantize.degree_to_radian.90000.truncate` | m° i32 -> urad i32 | 90000 | truncate | checked | 1570796 | ✅ | ✅ | ✅ | ✅ |
| `requantize.degree_to_radian.90000.floor` | m° i32 -> urad i32 | 90000 | floor | checked | 1570796 | ✅ | ✅ | ✅ | ✅ |
| `requantize.degree_to_radian.90000.ceil` | m° i32 -> urad i32 | 90000 | ceil | checked | 1570797 | ✅ | ✅ | ✅ | ✅ |
| `requantize.degree_to_radian.90000.nearest_even` | m° i32 -> urad i32 | 90000 | nearest_even | checked | 1570796 | ✅ | ✅ | ✅ | ✅ |
| `requantize.degree_to_radian.1.truncate` | m° i32 -> urad i32 | 1 | truncate | checked | 17 | ✅ | ✅ | ✅ | ✅ |
| `requantize.degree_to_radian.1.floor` | m° i32 -> urad i32 | 1 | floor | checked | 17 | ✅ | ✅ | ✅ | ✅ |
| `requantize.degree_to_radian.1.ceil` | m° i32 -> urad i32 | 1 | ceil | checked | 18 | ✅ | ✅ | ✅ | ✅ |
| `requantize.degree_to_radian.1.nearest_even` | m° i32 -> urad i32 | 1 | nearest_even | checked | 17 | ✅ | ✅ | ✅ | ✅ |
| `requantize.degree_to_radian.negative.floor` | m° i32 -> urad i32 | -45000 | floor | checked | -785399 | ✅ | ✅ | ✅ | ✅ |
| `requantize.degree_to_radian.negative.truncate` | m° i32 -> urad i32 | -45000 | truncate | checked | -785398 | ✅ | ✅ | ✅ | ✅ |
| `requantize.degree_to_radian_i64.big` | m° i64 -> urad i64 | 900000000000000 | nearest_even | checked | 15707963267948966 | ✅ | ✅ | ✅ | ✅ |
| `requantize.degree_to_radian_i64.past_top.checked` | m° i64 -> urad i64 | 2000000000000000000 | nearest_even | checked | none | ✅ | ✅ | ✅ | ✅ |
| `requantize.degree_to_radian_i64.past_top.saturate` | m° i64 -> urad i64 | 2000000000000000000 | nearest_even | saturate | 9223372036854775807 | ✅ | ✅ | ✅ | ✅ |
| `requantize.degree_to_radian_i64.past_bottom.saturate` | m° i64 -> urad i64 | -2000000000000000000 | floor | saturate | -9223372036854775808 | ✅ | ✅ | ✅ | ✅ |

## Gaps

- **c**: no generated C for this type yet: C from the ww_self model (API-26). 136 cases: `vec.u16`, `vec.array_vec`, `vec.string`, `vec.bool`, `vec.option`, `vec.vec`, `option.none`, `option.some`, `option.str`, `option.option.some_some`, `option.option.some_none`, `result.ok`, `result.err`, `array.u16`, `array.bool`, `tuple`, `range`, `range_inclusive`, `struct.bits`, `struct.nested`, `struct.flags.relocated`, `struct.flags.relocated_other`, `struct.tuple`, `struct.tuple.none`, `struct.tuple.plain`, `struct.unit`, `struct.sized`, `struct.final_structure`, `struct.generic`, `struct.phantom`, `struct.validate.ok`, `struct.validate.rejected`, `struct.tail_size`, `struct.generic_const`, `struct.sparse.empty`, `struct.sparse.slots`, `struct.recursive.list`, `struct.recursive.box`, `enum.unit`, `enum.tuple`, `enum.struct`, `enum.explicit_discriminant`, `enum.unknown_variant`, `enum.appended_variant`, `enum.sub_byte.seven`, `enum.sub_byte.ten`, `enum.sized`, `enum.sized.wide_discriminant`, `enum.u2`, `enum.nibble.unit`, `enum.nibble.payload`, `enum.cfg.kept`, `enum.cfg.kept_after_gap`, `enum.cfg.compiled_out`, `evolution.v1`, `evolution.v2`, `evolution.new_reads_old`, `evolution.old_reads_new`, `series.u16`, `series.struct`, `transparent.u64`, `transparent.unib32`, `transparent.fields.plain`, `transparent.fields`, `transparent.sized.plain`, `transparent.sized`, `transparent.vec.unib32.plain`, `transparent.vec.unib32`, `transparent.vec.nibble.plain`, `transparent.vec.nibble`, `transparent.option.plain`, `transparent.option`, `transparent.array.plain`, `transparent.array`, `transparent.series.plain`, `transparent.series`, `transparent.series.struct.plain`, `transparent.series.struct`, `wire.struct`, `wire.struct.out_of_range`, `wire.enum.tuple`, `wire.enum.named`, `wire.series`, `cfg_attr.v1`, `cfg_attr.v2`, `cfg_attr.new_reads_old`, `tail_bytes`, `stdlib.timebase.steer_point`, `stdlib.timebase.steer_point.steerer`, `stdlib.timebase.sync_status`, `stdlib.timebase.sync_status.steerers`, `stdlib.si.dim.volt`, `stdlib.si.dim.rad_bit`, `stdlib.si.dim.counted`, `stdlib.si.dim.rejected`, `stdlib.si.dim.root`, `stdlib.si.dim.root.rejected`, `stdlib.si.fixed.pow10`, `stdlib.si.fixed.duty16`, `stdlib.si.fixed.bounded`, `stdlib.si.fixed.bounded.rejected`, `stdlib.si.fixed.customary`, `stdlib.si.magnitude.one`, `stdlib.si.magnitude.milli`, `stdlib.si.magnitude.kibi`, `stdlib.si.magnitude.duty16`, `stdlib.si.magnitude.degree`, `stdlib.si.magnitude.sqrt2`, `stdlib.si.magnitude.decibel_in_neper`, `stdlib.si.magnitude.pound`, `stdlib.si.magnitude.rejected`, `stdlib.si.point.celsius`, `stdlib.si.point.bounded`, `stdlib.si.point.bounded.rejected`, `stdlib.si.index.in_struct`, `stdlib.si.index_from_1`, `stdlib.si.index_from_1.zero.rejected`, `stdlib.si.index_from_1.bounded.rejected`, `stdlib.si.seq.nibble_wrap`, `stdlib.si.offset.zero`, `stdlib.si.offset.celsius`, `stdlib.si.offset.minus_forty`, `stdlib.si.offset.fahrenheit`, `stdlib.si.offset.rejected`, `stdlib.si.si_expr.volt`, `stdlib.si.si_expr.pound_force`, `stdlib.si.si_expr.sqrt`, `stdlib.si.si_expr.rejected`, `model.marker_def.unit`, `model.marker_def.unit_conventional`, `model.marker_def.scale`, `model.marker_def.per`, `model.marker_def.origin`, `model.marker_def.angle`, `model.item_struct_extra.transparent`, `model.item_struct_extra.marker`.
- **c**: no generated C for this type yet: C from the ww_self model (API-26). 135 cases: `vec.u16`, `vec.array_vec`, `vec.string`, `vec.bool`, `vec.option`, `vec.vec`, `option.none`, `option.some`, `option.str`, `option.option.some_some`, `option.option.some_none`, `result.ok`, `result.err`, `array.u16`, `array.bool`, `tuple`, `range`, `range_inclusive`, `struct.bits`, `struct.nested`, `struct.flags.relocated`, `struct.flags.relocated_other`, `struct.tuple`, `struct.tuple.none`, `struct.tuple.plain`, `struct.unit`, `struct.sized`, `struct.final_structure`, `struct.generic`, `struct.phantom`, `struct.validate.ok`, `struct.validate.rejected`, `struct.tail_size`, `struct.generic_const`, `struct.sparse.empty`, `struct.sparse.slots`, `struct.recursive.list`, `struct.recursive.box`, `enum.unit`, `enum.tuple`, `enum.struct`, `enum.explicit_discriminant`, `enum.unknown_variant`, `enum.appended_variant`, `enum.sub_byte.seven`, `enum.sub_byte.ten`, `enum.sized`, `enum.sized.wide_discriminant`, `enum.u2`, `enum.nibble.unit`, `enum.nibble.payload`, `enum.cfg.kept`, `enum.cfg.kept_after_gap`, `enum.cfg.compiled_out`, `evolution.v1`, `evolution.v2`, `evolution.new_reads_old`, `evolution.old_reads_new`, `series.u16`, `series.struct`, `transparent.u64`, `transparent.unib32`, `transparent.fields.plain`, `transparent.fields`, `transparent.sized.plain`, `transparent.sized`, `transparent.vec.unib32.plain`, `transparent.vec.unib32`, `transparent.vec.nibble.plain`, `transparent.vec.nibble`, `transparent.option.plain`, `transparent.option`, `transparent.array.plain`, `transparent.array`, `transparent.series.plain`, `transparent.series`, `transparent.series.struct.plain`, `transparent.series.struct`, `tail_bytes`, `stdlib.timebase.steer_point`, `stdlib.timebase.steer_point.steerer`, `stdlib.timebase.sync_status`, `stdlib.timebase.sync_status.steerers`, `stdlib.si.dim.volt`, `stdlib.si.dim.rad_bit`, `stdlib.si.dim.counted`, `stdlib.si.dim.rejected`, `stdlib.si.dim.root`, `stdlib.si.dim.root.rejected`, `stdlib.si.fixed.pow10`, `stdlib.si.fixed.duty16`, `stdlib.si.fixed.bounded`, `stdlib.si.fixed.bounded.rejected`, `stdlib.si.fixed.customary`, `stdlib.si.quantity.milli`, `stdlib.si.quantity.kibi`, `stdlib.si.quantity.exact.duty16`, `stdlib.si.quantity.exact.pow10`, `stdlib.si.quantity.exact.bam`, `stdlib.si.quantity.bounded`, `stdlib.si.quantity.bounded.rejected`, `stdlib.si.magnitude.one`, `stdlib.si.magnitude.milli`, `stdlib.si.magnitude.kibi`, `stdlib.si.magnitude.duty16`, `stdlib.si.magnitude.degree`, `stdlib.si.magnitude.sqrt2`, `stdlib.si.magnitude.decibel_in_neper`, `stdlib.si.magnitude.pound`, `stdlib.si.magnitude.rejected`, `stdlib.si.point.celsius`, `stdlib.si.point.bounded`, `stdlib.si.point.bounded.rejected`, `stdlib.si.index.in_struct`, `stdlib.si.index_from_1`, `stdlib.si.index_from_1.zero.rejected`, `stdlib.si.index_from_1.bounded.rejected`, `stdlib.si.seq.nibble_wrap`, `stdlib.si.offset.zero`, `stdlib.si.offset.celsius`, `stdlib.si.offset.minus_forty`, `stdlib.si.offset.fahrenheit`, `stdlib.si.offset.rejected`, `stdlib.si.si_expr.volt`, `stdlib.si.si_expr.pound_force`, `stdlib.si.si_expr.sqrt`, `stdlib.si.si_expr.rejected`, `model.marker_def.unit`, `model.marker_def.unit_conventional`, `model.marker_def.scale`, `model.marker_def.per`, `model.marker_def.origin`, `model.marker_def.angle`, `model.item_struct_extra.transparent`, `model.item_struct_extra.marker`.
- **c**: no generated C for this type yet: C from the ww_self model (API-26). 135 cases: `vec.u16`, `vec.array_vec`, `vec.string`, `vec.bool`, `vec.option`, `vec.vec`, `option.none`, `option.some`, `option.str`, `option.option.some_some`, `option.option.some_none`, `result.ok`, `result.err`, `array.u16`, `array.bool`, `tuple`, `range`, `range_inclusive`, `struct.bits`, `struct.nested`, `struct.flags.relocated`, `struct.flags.relocated_other`, `struct.tuple`, `struct.tuple.none`, `struct.tuple.plain`, `struct.unit`, `struct.sized`, `struct.final_structure`, `struct.generic`, `struct.phantom`, `struct.validate.ok`, `struct.validate.rejected`, `struct.tail_size`, `struct.generic_const`, `struct.sparse.empty`, `struct.sparse.slots`, `struct.recursive.list`, `struct.recursive.box`, `enum.unit`, `enum.tuple`, `enum.struct`, `enum.explicit_discriminant`, `enum.unknown_variant`, `enum.appended_variant`, `enum.sub_byte.seven`, `enum.sub_byte.ten`, `enum.sized`, `enum.sized.wide_discriminant`, `enum.u2`, `enum.nibble.unit`, `enum.nibble.payload`, `enum.cfg.kept`, `enum.cfg.kept_after_gap`, `enum.cfg.compiled_out`, `evolution.v1`, `evolution.v2`, `evolution.new_reads_old`, `evolution.old_reads_new`, `series.u16`, `series.struct`, `transparent.u64`, `transparent.unib32`, `transparent.fields.plain`, `transparent.fields`, `transparent.sized.plain`, `transparent.sized`, `transparent.vec.unib32.plain`, `transparent.vec.unib32`, `transparent.vec.nibble.plain`, `transparent.vec.nibble`, `transparent.option.plain`, `transparent.option`, `transparent.array.plain`, `transparent.array`, `transparent.series.plain`, `transparent.series`, `transparent.series.struct.plain`, `transparent.series.struct`, `tail_bytes`, `stdlib.timebase.steer_point`, `stdlib.timebase.steer_point.steerer`, `stdlib.timebase.sync_status`, `stdlib.timebase.sync_status.steerers`, `stdlib.si.dim.volt`, `stdlib.si.dim.rad_bit`, `stdlib.si.dim.counted`, `stdlib.si.dim.rejected`, `stdlib.si.dim.root`, `stdlib.si.dim.root.rejected`, `stdlib.si.fixed.pow10`, `stdlib.si.fixed.duty16`, `stdlib.si.fixed.bounded`, `stdlib.si.fixed.bounded.rejected`, `stdlib.si.fixed.customary`, `stdlib.si.quantity.milli`, `stdlib.si.quantity.kibi`, `stdlib.si.quantity.pow10`, `stdlib.si.quantity.pow2`, `stdlib.si.quantity.fullscale`, `stdlib.si.quantity.bounded`, `stdlib.si.quantity.bounded.rejected`, `stdlib.si.magnitude.one`, `stdlib.si.magnitude.milli`, `stdlib.si.magnitude.kibi`, `stdlib.si.magnitude.duty16`, `stdlib.si.magnitude.degree`, `stdlib.si.magnitude.sqrt2`, `stdlib.si.magnitude.decibel_in_neper`, `stdlib.si.magnitude.pound`, `stdlib.si.magnitude.rejected`, `stdlib.si.point.celsius`, `stdlib.si.point.bounded`, `stdlib.si.point.bounded.rejected`, `stdlib.si.index.in_struct`, `stdlib.si.index_from_1`, `stdlib.si.index_from_1.zero.rejected`, `stdlib.si.index_from_1.bounded.rejected`, `stdlib.si.seq.nibble_wrap`, `stdlib.si.offset.zero`, `stdlib.si.offset.celsius`, `stdlib.si.offset.minus_forty`, `stdlib.si.offset.fahrenheit`, `stdlib.si.offset.rejected`, `stdlib.si.si_expr.volt`, `stdlib.si.si_expr.pound_force`, `stdlib.si.si_expr.sqrt`, `stdlib.si.si_expr.rejected`, `model.marker_def.unit`, `model.marker_def.unit_conventional`, `model.marker_def.scale`, `model.marker_def.per`, `model.marker_def.origin`, `model.marker_def.angle`, `model.item_struct_extra.transparent`, `model.item_struct_extra.marker`.
- **c**: no unit conversions in C yet: Fixed in C (CLT-54). 72 cases: `requantize.milli_to_deci.tie.truncate`, `requantize.milli_to_deci.tie.floor`, `requantize.milli_to_deci.tie.ceil`, `requantize.milli_to_deci.tie.nearest_even`, `requantize.milli_to_deci.tie.nearest_away`, `requantize.milli_to_deci.negative_tie.truncate`, `requantize.milli_to_deci.negative_tie.floor`, `requantize.milli_to_deci.negative_tie.ceil`, `requantize.milli_to_deci.negative_tie.nearest_even`, `requantize.milli_to_deci.negative_tie.nearest_away`, `requantize.milli_to_deci.tie_up_to_even`, `requantize.milli_to_deci.below_tie`, `requantize.milli_to_deci.overflow.checked`, `requantize.milli_to_deci.overflow.saturate`, `requantize.milli_to_deci.overflow.wrap`, `requantize.milli_to_deci.underflow.saturate`, `requantize.micro_to_milli_unsigned.negative.checked`, `requantize.micro_to_milli_unsigned.negative.saturate`, `requantize.micro_to_milli_unsigned.negative.wrap`, `requantize.micro_to_milli_unsigned.top`, `requantize.micro_to_milli_unsigned.tie_past_top.checked`, `requantize.micro_to_milli_unsigned.tie_past_top.saturate`, `requantize.q15_to_q7.tie_past_top.checked`, `requantize.q15_to_q7.tie_past_top.saturate`, `requantize.q15_to_q7.tie.nearest_even`, `requantize.q15_to_q7.tie.nearest_away`, `requantize.q15_to_q7.negative_end`, `requantize.seven_thirds.truncate`, `requantize.seven_thirds.ceil`, `requantize.seven_thirds.nearest_even`, `requantize.seven_thirds.negative.floor`, `requantize.seven_thirds.negative.truncate`, `requantize.seven_thirds.exact_top`, `requantize.seven_thirds.past_top.checked`, `requantize.seven_thirds.past_top.saturate`, `requantize.seven_thirds.past_top.wrap`, `requantize.milli_to_micro.fits`, `requantize.milli_to_micro.overflow.checked`, `requantize.milli_to_micro.overflow.saturate`, `requantize.milli_to_micro.overflow.wrap`, `requantize.bam16_to_bam8.truncate`, `requantize.bam16_to_bam8.full_turn.checked`, `requantize.bam16_to_bam8.full_turn.saturate`, `requantize.bam16_to_bam8.full_turn.wrap`, `requantize.bam16_to_bam8.tie`, `requantize.float_to_int.tie.nearest_even`, `requantize.float_to_int.tie.nearest_away`, `requantize.float_to_int.negative_tie.nearest_even`, `requantize.float_to_int.negative.floor`, `requantize.float_to_int.tie_past_top.checked`, `requantize.float_to_int.tie_past_top.saturate`, `requantize.float_to_int.huge.saturate`, `requantize.float_to_int.huge.wrap`, `requantize.int_to_float.milli`, `requantize.degree_to_radian.45000.truncate`, `requantize.degree_to_radian.45000.floor`, `requantize.degree_to_radian.45000.ceil`, `requantize.degree_to_radian.45000.nearest_even`, `requantize.degree_to_radian.90000.truncate`, `requantize.degree_to_radian.90000.floor`, `requantize.degree_to_radian.90000.ceil`, `requantize.degree_to_radian.90000.nearest_even`, `requantize.degree_to_radian.1.truncate`, `requantize.degree_to_radian.1.floor`, `requantize.degree_to_radian.1.ceil`, `requantize.degree_to_radian.1.nearest_even`, `requantize.degree_to_radian.negative.floor`, `requantize.degree_to_radian.negative.truncate`, `requantize.degree_to_radian_i64.big`, `requantize.degree_to_radian_i64.past_top.checked`, `requantize.degree_to_radian_i64.past_top.saturate`, `requantize.degree_to_radian_i64.past_bottom.saturate`.
- **c**: reader only: the generated C has no writer yet (API-26). 8 cases: `fixed.numbers`, `fixed.bits`, `fixed.nested`, `fixed.option.none`, `fixed.option.some`, `fixed.enum.unit`, `fixed.enum.fields`, `head_tail.struct`.
- **c**: the C runtime has no 128-bit numbers (SW-66). 2 cases: `int.u128.max`, `int.i128.min`.
- **c**: ww_link's Disconnect payload, not a type of an API in the model. 2 cases: `disconnect_reason.hub_handover`, `disconnect_reason.from_newer`.
- **c**: ww_link's LinkSetup is a link message, not a type of an API in the model. 2 cases: `link_setup.without_meta`, `link_setup.with_meta`.
- **dynamic**: TailBytes has no form in the API model yet (INT-9). 1 case: `tail_bytes`.
- **dynamic**: TailSize has no form in the API model yet (INT-9). 1 case: `struct.tail_size`.
- **dynamic**: a validate function is Rust code the API model can't carry; checks in the model come with sub-type numbers (SW-20). 5 cases: `struct.validate.rejected`, `stdlib.si.dim.rejected`, `stdlib.si.dim.root.rejected`, `stdlib.si.magnitude.rejected`, `stdlib.si.offset.rejected`.
- **dynamic**: bounds are ww_si markers in the type arguments, checked by ww_si-aware clients (CLT-54). 4 cases: `stdlib.si.fixed.bounded.rejected`, `stdlib.si.quantity.bounded.rejected`, `stdlib.si.point.bounded.rejected`, `stdlib.si.index_from_1.bounded.rejected`.
- **dynamic**: by design: the API model has the variants of every build, a host reads it (DER-8). 1 case: `enum.cfg.compiled_out`.
- **dynamic**: head_tail is a wire format of its own without ShrinkWrap serdes, not an API type (SW-46). 1 case: `head_tail.struct`.
- **dynamic**: null is None and Some(None) alike in the corpus value form, as in Python (TOOL-13). 1 case: `option.option.some_none`.
- **dynamic**: the API model has no #[default] values, an old field reads as its type's default (INT-10). 2 cases: `evolution.new_reads_old`, `cfg_attr.new_reads_old`.
- **dynamic**: ww_link's Disconnect payload, not a type of an API in the model; a dynamic consumer reads API types (SW-46 is the same for head_tail). 2 cases: `disconnect_reason.hub_handover`, `disconnect_reason.from_newer`.
- **dynamic**: ww_link's LinkSetup is a link message, not a type of an API in the model; a dynamic consumer reads API types (SW-46 is the same for head_tail). 2 cases: `link_setup.without_meta`, `link_setup.with_meta`.
- **explain**: TailBytes has no form in the API model yet (INT-9). 1 case: `tail_bytes`.
- **explain**: TailSize has no form in the API model yet (INT-9). 1 case: `struct.tail_size`.
- **explain**: a validate function is Rust code the API model can't carry; checks in the model come with sub-type numbers (SW-20). 5 cases: `struct.validate.rejected`, `stdlib.si.dim.rejected`, `stdlib.si.dim.root.rejected`, `stdlib.si.magnitude.rejected`, `stdlib.si.offset.rejected`.
- **explain**: bounds are ww_si markers in the type arguments, checked by ww_si-aware clients (CLT-54). 4 cases: `stdlib.si.fixed.bounded.rejected`, `stdlib.si.quantity.bounded.rejected`, `stdlib.si.point.bounded.rejected`, `stdlib.si.index_from_1.bounded.rejected`.
- **explain**: by design: the API model has the variants of every build, a host reads it (DER-8). 1 case: `enum.cfg.compiled_out`.
- **explain**: head_tail is a wire format of its own without ShrinkWrap serdes, not an API type (SW-46). 1 case: `head_tail.struct`.
- **explain**: null is None and Some(None) alike in the corpus value form, as in Python (TOOL-13). 1 case: `option.option.some_none`.
- **explain**: the API model has no #[default] values, an old field reads as its type's default (INT-10). 2 cases: `evolution.new_reads_old`, `cfg_attr.new_reads_old`.
- **explain**: ww_link's Disconnect payload, not a type of an API in the model. 2 cases: `disconnect_reason.hub_handover`, `disconnect_reason.from_newer`.
- **explain**: ww_link's LinkSetup is a link message, not a type of an API in the model. 2 cases: `link_setup.without_meta`, `link_setup.with_meta`.
- **python**: TailBytes has no form in the API model yet (INT-9). 1 case: `tail_bytes`.
- **python**: TailSize has no form in the API model yet (INT-9). 1 case: `struct.tail_size`.
- **python**: a validate function is Rust code the API model can't carry; checks in the model come with sub-type numbers (SW-20). 5 cases: `struct.validate.rejected`, `stdlib.si.dim.rejected`, `stdlib.si.dim.root.rejected`, `stdlib.si.magnitude.rejected`, `stdlib.si.offset.rejected`.
- **python**: bounds are ww_si markers in the type arguments, checked by ww_si-aware clients (CLT-54). 4 cases: `stdlib.si.fixed.bounded.rejected`, `stdlib.si.quantity.bounded.rejected`, `stdlib.si.point.bounded.rejected`, `stdlib.si.index_from_1.bounded.rejected`.
- **python**: by design: the API model has the variants of every build (DER-8). 1 case: `enum.cfg.compiled_out`.
- **python**: head_tail is a wire format of its own without ShrinkWrap serdes, not an API type (SW-46). 1 case: `head_tail.struct`.
- **python**: null is None and Some(None) alike in the Python value form (TOOL-13). 1 case: `option.option.some_none`.
- **python**: the API model has no #[default] values (INT-10). 2 cases: `evolution.new_reads_old`, `cfg_attr.new_reads_old`.
- **python**: ww_link's Disconnect payload, not a type of an API in the model. 2 cases: `disconnect_reason.hub_handover`, `disconnect_reason.from_newer`.
- **python**: ww_link's LinkSetup is a link message, not a type of an API in the model. 2 cases: `link_setup.without_meta`, `link_setup.with_meta`.
