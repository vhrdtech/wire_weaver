---
level: nomicon
---

# Conformance matrix

<!-- Generated from tests/conformance/corpus.json by `just conformance-update`, do not edit. -->

Every case of the [conformance corpus](conformance.md) and what each consumer does with it: ✅ encodes to the golden bytes and decodes them back to the value (or only decodes them, or rejects them, as the case's direction says); a gap names why the consumer can't run the case yet and the item that fixes it, *reader only* that it decodes but can't encode, *by design* that it behaves otherwise on purpose. The tests fail when a mark is missing or the consumer passes a marked case.

| Consumer | Cases run | Gaps |
|---|---|---|
| derive | 290 of 290 | 0 |
| dynamic | 247 of 290 | 43 |
| explain | 247 of 290 | 43 |
| python | 245 of 290 | 45 |
| c | 34 of 290 | 256 |

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
| `inib32.one_nibble` | `inib32` | both | ✅ | ✅ | ✅ | ✅ | gap SW-75 |
| `inib32.two_nibbles` | `inib32` | both | ✅ | ✅ | ✅ | ✅ | gap SW-75 |
| `inib32.min` | `inib32` | both | ✅ | ✅ | ✅ | ✅ | gap SW-75 |
| `inib32.max` | `inib32` | both | ✅ | ✅ | ✅ | ✅ | gap SW-75 |
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
| `transparent.inib32` | `signed_index` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `transparent.fields.plain` | `plain_fields` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `transparent.fields` | `wrapped_fields` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `transparent.sized.plain` | `plain_sized` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `transparent.sized` | `wrapped_sized` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `transparent.vec.unib32.plain` | `vec_unib32` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `transparent.vec.unib32` | `vec_index` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `transparent.vec.inib32.plain` | `vec_inib32` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `transparent.vec.inib32` | `vec_signed_index` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
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
| `wire.struct.signed` | `wired_signed` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `wire.struct.signed.out_of_range` | `wired_signed` | reject value | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `wire.enum.tuple` | `wired_op` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `wire.enum.named` | `wired_op` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `wire.series` | `series_wired` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `cfg_attr.v1` | `gated_v1` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `cfg_attr.v2` | `gated_v2` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `cfg_attr.new_reads_old` | `gated_v2` | decode | ✅ | gap INT-10 | gap INT-10 | gap INT-10 | gap API-26 |
| `head_tail.struct` | `HtRecord` | both | ✅ | gap SW-46 | gap SW-46 | gap SW-46 | reader only (API-26) |
| `device_info.ordinary` | `DeviceInfo` | decode | ✅ | gap - | gap - | gap - | gap - |
| `device_info.no_observe_samples` | `DeviceInfo` | decode | ✅ | gap - | gap - | gap - | gap - |
| `device_info.wire_version` | `DeviceInfo` | both | ✅ | gap - | gap - | gap - | gap - |
| `link_setup.without_meta` | `LinkSetup` | both | ✅ | gap - | gap - | gap - | gap - |
| `link_setup.wire_version` | `LinkSetup` | both | ✅ | gap - | gap - | gap - | gap - |
| `link_setup.with_meta` | `LinkSetup` | both | ✅ | gap - | gap - | gap - | gap - |
| `client_server.stream_open.compressed` | `RequestKind` | both | ✅ | gap - | gap - | gap - | gap - |
| `client_server.stream_open.raw` | `RequestKind` | both | ✅ | gap - | gap - | gap - | gap - |
| `client_server.stream_opened.lz4` | `StreamOpened` | both | ✅ | gap - | gap - | gap - | gap - |
| `client_server.stream_opened.unknown` | `StreamOpened` | both | ✅ | gap - | gap - | gap - | gap - |
| `disconnect_reason.hub_handover` | `DisconnectReason` | both | ✅ | gap - | gap - | gap - | gap - |
| `disconnect_reason.from_newer` | `DisconnectReason` | both | ✅ | gap - | gap - | gap - | gap - |
| `tail_bytes` | `WithTail` | both | ✅ | gap INT-9 | gap INT-9 | gap INT-9 | gap API-26 |
| `client_server.request.multi_read` | `Request` | both | ✅ | gap - | gap - | gap - | gap - |
| `client_server.request.sink_write` | `Request` | both | ✅ | gap - | gap - | gap - | gap - |
| `client_server.request.sideband_ack` | `Request` | both | ✅ | gap - | gap - | gap - | gap - |
| `client_server.request.absolute_path` | `Request` | both | ✅ | gap - | gap - | gap - | gap - |
| `client_server.request.global_path` | `Request` | both | ✅ | gap - | gap - | gap - | gap - |
| `client_server.request.urgent` | `Request` | both | ✅ | gap - | gap - | gap - | gap - |
| `client_server.request.highest_reserved` | `Request` | both | ✅ | gap - | gap - | gap - | gap - |
| `client_server.event.stream_data_path` | `Event` | both | ✅ | gap - | gap - | gap - | gap - |
| `client_server.multi_index.list` | `MultiIndex` | both | ✅ | gap - | gap - | gap - | gap - |
| `stdlib.timebase.steer_point` | `steer_point` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.timebase.steer_point.steerer` | `steer_point` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.timebase.sync_status` | `sync_status` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.timebase.sync_status.steerers` | `sync_status` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.dim.volt` | `dim` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.dim.rad_bit` | `dim` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.dim.counted` | `dim` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.dim.six_bases` | `dim` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.dim.rejected` | `dim` | reject | ✅ | gap SW-20 | gap SW-20 | gap SW-20 | gap API-26 |
| `stdlib.si.dim.zero.rejected` | `dim` | reject | ✅ | gap SW-20 | gap SW-20 | gap SW-20 | gap API-26 |
| `stdlib.si.dim.count.rejected` | `dim` | reject | ✅ | gap SW-20 | gap SW-20 | gap SW-20 | gap API-26 |
| `stdlib.si.dim.root` | `dim` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.dim.gaussian_half` | `dim` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.dim.gaussian.count.rejected` | `dim` | reject | ✅ | gap SW-20 | gap SW-20 | gap SW-20 | gap API-26 |
| `stdlib.si.dim.gaussian.ratio.rejected` | `dim` | reject | ✅ | gap SW-20 | gap SW-20 | gap SW-20 | gap API-26 |
| `stdlib.si.fixed.pow10` | `fixed_pow10` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.fixed.duty16` | `duty16` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.coded.mu_law.loudest` | `coded_mu_law` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.coded.mu_law.loudest_negative` | `coded_mu_law` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.coded.mu_law.zero` | `coded_mu_law` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.coded.mu_law.negative_zero` | `coded_mu_law` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.coded.mu_law.segment_0_top` | `coded_mu_law` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.coded.mu_law.segment_1_bottom` | `coded_mu_law` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.coded.mu_law_single_zero.zero` | `coded_mu_law_single_zero` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.coded.mu_law_single_zero.loudest` | `coded_mu_law_single_zero` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.coded.mu_law_single_zero.spare.rejected` | `coded_mu_law_single_zero` | reject | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.niched.mu_law.value` | `niched_mu_law` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.niched.mu_law.niche` | `niched_mu_law` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.fixed.bounded` | `fixed_bounded` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.fixed.bounded.rejected` | `fixed_bounded` | reject | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.fixed.f32` | `fixed_f32` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.fixed.f32.inf` | `fixed_f32` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.fixed.f32.nan.rejected` | `fixed_f32` | reject | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.fixed.finite` | `fixed_finite` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.fixed.finite.inf.rejected` | `fixed_finite` | reject | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.fixed.finite.nan.rejected` | `fixed_finite` | reject | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.fixed.maybe_nan.nan` | `fixed_maybe_nan` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.fixed.maybe_nan.number` | `fixed_maybe_nan` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.fixed.customary` | `fixed_customary` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.quantity.milli` | `quantity_volt` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.quantity.kibi` | `quantity_byte` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.quantity.pow10` | `quantity_second` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.quantity.pow2` | `quantity_second` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.quantity.fullscale` | `quantity_ratio` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.quantity.unknown_scale` | `quantity_ratio` | decode | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.quantity.bounded` | `quantity_rail` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.quantity.bounded.rejected` | `quantity_rail` | reject | ✅ | gap CLT-54 | gap CLT-54 | gap CLT-54 | gap API-26 |
| `stdlib.si.quantity.bounded.micro` | `quantity_rail` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.quantity.bounded.micro.rejected` | `quantity_rail` | reject | ✅ | gap CLT-54 | gap CLT-54 | gap CLT-54 | gap API-26 |
| `stdlib.si.quantity.bounded.pow2` | `quantity_rail` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.quantity.bounded.pow2.rejected` | `quantity_rail` | reject | ✅ | gap CLT-54 | gap CLT-54 | gap CLT-54 | gap API-26 |
| `stdlib.si.quantity.bounded.fullscale` | `quantity_rail` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.quantity.bounded.fullscale.rejected` | `quantity_rail` | reject | ✅ | gap CLT-54 | gap CLT-54 | gap CLT-54 | gap API-26 |
| `stdlib.si.quantity.bounded.f32` | `quantity_rail` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.quantity.bounded.f32.rejected` | `quantity_rail` | reject | ✅ | gap CLT-54 | gap CLT-54 | gap CLT-54 | gap API-26 |
| `stdlib.si.magnitude.one` | `magnitude` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.magnitude.milli` | `magnitude` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.magnitude.kibi` | `magnitude` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.magnitude.duty16` | `magnitude` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.magnitude.degree` | `magnitude` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.magnitude.sqrt2` | `magnitude` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.magnitude.decibel_in_neper` | `magnitude` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.magnitude.pound` | `magnitude` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.magnitude.rejected` | `magnitude` | reject | ✅ | gap SW-20 | gap SW-20 | gap SW-20 | gap API-26 |
| `stdlib.si.magnitude.atomic_mass_constant` | `magnitude` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.magnitude.wide_prime` | `magnitude` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.fraction.exp` | `fraction` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.fraction.negative` | `fraction` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.fraction.third` | `fraction` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.fraction.rejected` | `fraction` | reject | ✅ | gap SW-20 | gap SW-20 | gap SW-20 | gap API-26 |
| `stdlib.si.run_scale.ntsc` | `run_scale` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.run_scale.rejected` | `run_scale` | reject | ✅ | gap SW-20 | gap SW-20 | gap SW-20 | gap API-26 |
| `stdlib.si.ticks` | `ticks` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.point.celsius` | `point_celsius` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.point.bounded` | `point_bounded` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.point.bounded.rejected` | `point_bounded` | reject | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.timed.default` | `si_timed` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.timed.millis_u16` | `si_timed_millis` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.timed.series` | `series_si_timed` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.timed.series.prefix_change` | `series_si_timed_ampere` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
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
| `stdlib.si.si_expr.decibel_loss_per_hundred_metres` | `si_expr` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.si_expr.ref` | `si_expr` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.si_expr.ref_per` | `si_expr` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `stdlib.si.any_quantity.def` | `any_quantity` | both | ✅ | ✅ | ✅ | gap CLT-67 | gap API-26 |
| `stdlib.si.any_quantity.ref` | `any_quantity` | both | ✅ | ✅ | ✅ | gap CLT-67 | gap API-26 |
| `stdlib.si.si_expr.rejected` | `si_expr` | reject | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.marker_def.unit` | `marker_def` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.marker_def.unit_conventional` | `marker_def` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.marker_def.unit_kind` | `marker_def` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.marker_def.base` | `marker_def` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.marker_def.scale` | `marker_def` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.marker_def.per` | `marker_def` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.marker_def.product` | `marker_def` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.marker_def.quotient` | `marker_def` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.marker_def.origin` | `marker_def` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.marker_def.origin_time_scale` | `marker_def` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.marker_def.codec` | `marker_def` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.marker_def.codec.spare` | `marker_def` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.marker_def.codec.offset` | `marker_def` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.marker_def.codec.curve` | `marker_def` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.marker_def.reference` | `marker_def` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.marker_def.reference_run_time` | `marker_def` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.marker_def.angle` | `marker_def` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.marker_def.maybe_nan` | `marker_def` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.marker_def.finite` | `marker_def` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.marker_def.of` | `marker_def` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.marker_def.quantity` | `marker_def` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.marker_def.quantity_kind` | `marker_def` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.marker_def.system` | `marker_def` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.item_struct_extra.transparent` | `item_struct_extra` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `model.item_struct_extra.marker` | `item_struct_extra` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `args.one` | `args_one` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `args.plain` | `args_plain` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `args.flatten` | `args_flatten` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `args.defaults.given` | `args_defaults` | both | ✅ | ✅ | ✅ | ✅ | gap API-26 |
| `args.defaults.left_out` | `args_defaults` | decode | ✅ | ✅ | ✅ | ✅ | gap API-26 |

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
| `requantize.int_to_float.milli_inexact.truncate` | mV i32 -> V f64 | 1 | truncate | checked | 0.0009999999999999998 | ✅ | ✅ | ✅ | ✅ |
| `requantize.int_to_float.milli_inexact.nearest_even` | mV i32 -> V f64 | 1 | nearest_even | checked | 0.001 | ✅ | ✅ | ✅ | ✅ |
| `requantize.int_to_f32.i32_max.truncate` | i32 -> f32 | 2147483647 | truncate | checked | 2147483520.0 | ✅ | ✅ | ✅ | ✅ |
| `requantize.int_to_f32.i32_max.nearest_even` | i32 -> f32 | 2147483647 | nearest_even | checked | 2147483648.0 | ✅ | ✅ | ✅ | ✅ |
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

## Signatures

The shape signature and the signature with docs of a definition (EVO-69), by which a device leaves it out of its introspection data and a host or snapshot finds it again. Only `ww_self` calculates them.

| Case | Definition | Shape | With docs |
|---|---|---|---|
| `signature.struct.docs` | `type Signed` | `7f0a980582914d0b` | `9dbe71ee06aa30b1` |
| `signature.enum.docs` | `type SignedLevel` | `b6ebb6bb2dc8bfda` | `98026ae936f1fc16` |
| `signature.struct.no_docs` | `type Unsigned` | `3875e6573d917a6f` | `3875e6573d917a6f` |

## Gaps

- **c**: no Op::INib32 in the C runtime yet, only UNib32 (SW-75). 4 cases: `inib32.one_nibble`, `inib32.two_nibbles`, `inib32.min`, `inib32.max`.
- **c**: no generated C for method arguments yet: C from the ww_self model (API-26). 5 cases: `args.one`, `args.plain`, `args.flatten`, `args.defaults.given`, `args.defaults.left_out`.
- **c**: no generated C for this type yet: C from the ww_self model (API-26). 216 cases: `vec.u16`, `vec.array_vec`, `vec.string`, `vec.bool`, `vec.option`, `vec.vec`, `option.none`, `option.some`, `option.str`, `option.option.some_some`, `option.option.some_none`, `result.ok`, `result.err`, `array.u16`, `array.bool`, `tuple`, `range`, `range_inclusive`, `struct.bits`, `struct.nested`, `struct.flags.relocated`, `struct.flags.relocated_other`, `struct.tuple`, `struct.tuple.none`, `struct.tuple.plain`, `struct.unit`, `struct.sized`, `struct.final_structure`, `struct.generic`, `struct.phantom`, `struct.validate.ok`, `struct.validate.rejected`, `struct.tail_size`, `struct.generic_const`, `struct.sparse.empty`, `struct.sparse.slots`, `struct.recursive.list`, `struct.recursive.box`, `enum.unit`, `enum.tuple`, `enum.struct`, `enum.explicit_discriminant`, `enum.unknown_variant`, `enum.appended_variant`, `enum.sub_byte.seven`, `enum.sub_byte.ten`, `enum.sized`, `enum.sized.wide_discriminant`, `enum.u2`, `enum.nibble.unit`, `enum.nibble.payload`, `enum.cfg.kept`, `enum.cfg.kept_after_gap`, `enum.cfg.compiled_out`, `evolution.v1`, `evolution.v2`, `evolution.new_reads_old`, `evolution.old_reads_new`, `series.u16`, `series.struct`, `transparent.u64`, `transparent.unib32`, `transparent.inib32`, `transparent.fields.plain`, `transparent.fields`, `transparent.sized.plain`, `transparent.sized`, `transparent.vec.unib32.plain`, `transparent.vec.unib32`, `transparent.vec.inib32.plain`, `transparent.vec.inib32`, `transparent.vec.nibble.plain`, `transparent.vec.nibble`, `transparent.option.plain`, `transparent.option`, `transparent.array.plain`, `transparent.array`, `transparent.series.plain`, `transparent.series`, `transparent.series.struct.plain`, `transparent.series.struct`, `wire.struct`, `wire.struct.out_of_range`, `wire.struct.signed`, `wire.struct.signed.out_of_range`, `wire.enum.tuple`, `wire.enum.named`, `wire.series`, `cfg_attr.v1`, `cfg_attr.v2`, `cfg_attr.new_reads_old`, `tail_bytes`, `stdlib.timebase.steer_point`, `stdlib.timebase.steer_point.steerer`, `stdlib.timebase.sync_status`, `stdlib.timebase.sync_status.steerers`, `stdlib.si.dim.volt`, `stdlib.si.dim.rad_bit`, `stdlib.si.dim.counted`, `stdlib.si.dim.six_bases`, `stdlib.si.dim.rejected`, `stdlib.si.dim.zero.rejected`, `stdlib.si.dim.count.rejected`, `stdlib.si.dim.root`, `stdlib.si.dim.gaussian_half`, `stdlib.si.dim.gaussian.count.rejected`, `stdlib.si.dim.gaussian.ratio.rejected`, `stdlib.si.fixed.pow10`, `stdlib.si.fixed.duty16`, `stdlib.si.coded.mu_law.loudest`, `stdlib.si.coded.mu_law.loudest_negative`, `stdlib.si.coded.mu_law.zero`, `stdlib.si.coded.mu_law.negative_zero`, `stdlib.si.coded.mu_law.segment_0_top`, `stdlib.si.coded.mu_law.segment_1_bottom`, `stdlib.si.coded.mu_law_single_zero.zero`, `stdlib.si.coded.mu_law_single_zero.loudest`, `stdlib.si.coded.mu_law_single_zero.spare.rejected`, `stdlib.si.niched.mu_law.value`, `stdlib.si.niched.mu_law.niche`, `stdlib.si.fixed.bounded`, `stdlib.si.fixed.bounded.rejected`, `stdlib.si.fixed.f32`, `stdlib.si.fixed.f32.inf`, `stdlib.si.fixed.f32.nan.rejected`, `stdlib.si.fixed.finite`, `stdlib.si.fixed.finite.inf.rejected`, `stdlib.si.fixed.finite.nan.rejected`, `stdlib.si.fixed.maybe_nan.nan`, `stdlib.si.fixed.maybe_nan.number`, `stdlib.si.fixed.customary`, `stdlib.si.quantity.milli`, `stdlib.si.quantity.kibi`, `stdlib.si.quantity.pow10`, `stdlib.si.quantity.pow2`, `stdlib.si.quantity.fullscale`, `stdlib.si.quantity.unknown_scale`, `stdlib.si.quantity.bounded`, `stdlib.si.quantity.bounded.rejected`, `stdlib.si.quantity.bounded.micro`, `stdlib.si.quantity.bounded.micro.rejected`, `stdlib.si.quantity.bounded.pow2`, `stdlib.si.quantity.bounded.pow2.rejected`, `stdlib.si.quantity.bounded.fullscale`, `stdlib.si.quantity.bounded.fullscale.rejected`, `stdlib.si.quantity.bounded.f32`, `stdlib.si.quantity.bounded.f32.rejected`, `stdlib.si.magnitude.one`, `stdlib.si.magnitude.milli`, `stdlib.si.magnitude.kibi`, `stdlib.si.magnitude.duty16`, `stdlib.si.magnitude.degree`, `stdlib.si.magnitude.sqrt2`, `stdlib.si.magnitude.decibel_in_neper`, `stdlib.si.magnitude.pound`, `stdlib.si.magnitude.rejected`, `stdlib.si.magnitude.atomic_mass_constant`, `stdlib.si.magnitude.wide_prime`, `stdlib.si.fraction.exp`, `stdlib.si.fraction.negative`, `stdlib.si.fraction.third`, `stdlib.si.fraction.rejected`, `stdlib.si.run_scale.ntsc`, `stdlib.si.run_scale.rejected`, `stdlib.si.ticks`, `stdlib.si.point.celsius`, `stdlib.si.point.bounded`, `stdlib.si.point.bounded.rejected`, `stdlib.si.timed.default`, `stdlib.si.timed.millis_u16`, `stdlib.si.timed.series`, `stdlib.si.timed.series.prefix_change`, `stdlib.si.index.in_struct`, `stdlib.si.index_from_1`, `stdlib.si.index_from_1.zero.rejected`, `stdlib.si.index_from_1.bounded.rejected`, `stdlib.si.seq.nibble_wrap`, `stdlib.si.offset.zero`, `stdlib.si.offset.celsius`, `stdlib.si.offset.minus_forty`, `stdlib.si.offset.fahrenheit`, `stdlib.si.offset.rejected`, `stdlib.si.si_expr.volt`, `stdlib.si.si_expr.pound_force`, `stdlib.si.si_expr.sqrt`, `stdlib.si.si_expr.decibel_loss_per_hundred_metres`, `stdlib.si.si_expr.ref`, `stdlib.si.si_expr.ref_per`, `stdlib.si.any_quantity.def`, `stdlib.si.any_quantity.ref`, `stdlib.si.si_expr.rejected`, `model.marker_def.unit`, `model.marker_def.unit_conventional`, `model.marker_def.unit_kind`, `model.marker_def.base`, `model.marker_def.scale`, `model.marker_def.per`, `model.marker_def.product`, `model.marker_def.quotient`, `model.marker_def.origin`, `model.marker_def.origin_time_scale`, `model.marker_def.codec`, `model.marker_def.codec.spare`, `model.marker_def.codec.offset`, `model.marker_def.codec.curve`, `model.marker_def.reference`, `model.marker_def.reference_run_time`, `model.marker_def.angle`, `model.marker_def.maybe_nan`, `model.marker_def.finite`, `model.marker_def.of`, `model.marker_def.quantity`, `model.marker_def.quantity_kind`, `model.marker_def.system`, `model.item_struct_extra.transparent`, `model.item_struct_extra.marker`.
- **c**: reader only: the generated C has no writer yet (API-26). 8 cases: `fixed.numbers`, `fixed.bits`, `fixed.nested`, `fixed.option.none`, `fixed.option.some`, `fixed.enum.unit`, `fixed.enum.fields`, `head_tail.struct`.
- **c**: the C runtime has no 128-bit numbers (SW-66). 2 cases: `int.u128.max`, `int.i128.min`.
- **c**: ww_client_server's Event is a protocol message, not a type of an API in the model. 1 case: `client_server.event.stream_data_path`.
- **c**: ww_client_server's MultiIndex is a protocol message, not a type of an API in the model. 1 case: `client_server.multi_index.list`.
- **c**: ww_client_server's Request is a protocol message, not a type of an API in the model. 4 cases: `client_server.request.absolute_path`, `client_server.request.global_path`, `client_server.request.urgent`, `client_server.request.highest_reserved`.
- **c**: ww_client_server's RequestKind is the request envelope, not a type of an API in the model. 2 cases: `client_server.stream_open.compressed`, `client_server.stream_open.raw`.
- **c**: ww_client_server's StreamOpened is the answer envelope of a stream open, not a type of an API in the model. 2 cases: `client_server.stream_opened.lz4`, `client_server.stream_opened.unknown`.
- **c**: ww_client_server's request envelope, not a type of an API in the model. 3 cases: `client_server.request.multi_read`, `client_server.request.sink_write`, `client_server.request.sideband_ack`.
- **c**: ww_link's DeviceInfo is a link message, not a type of an API in the model. 3 cases: `device_info.ordinary`, `device_info.no_observe_samples`, `device_info.wire_version`.
- **c**: ww_link's Disconnect payload, not a type of an API in the model. 2 cases: `disconnect_reason.hub_handover`, `disconnect_reason.from_newer`.
- **c**: ww_link's LinkSetup is a link message, not a type of an API in the model. 3 cases: `link_setup.without_meta`, `link_setup.wire_version`, `link_setup.with_meta`.
- **dynamic**: TailBytes has no form in the API model yet (INT-9). 1 case: `tail_bytes`.
- **dynamic**: TailSize has no form in the API model yet (INT-9). 1 case: `struct.tail_size`.
- **dynamic**: a validate function is Rust code the API model can't carry; checks in the model come with sub-type numbers (SW-20). 10 cases: `struct.validate.rejected`, `stdlib.si.dim.rejected`, `stdlib.si.dim.zero.rejected`, `stdlib.si.dim.count.rejected`, `stdlib.si.dim.gaussian.count.rejected`, `stdlib.si.dim.gaussian.ratio.rejected`, `stdlib.si.magnitude.rejected`, `stdlib.si.fraction.rejected`, `stdlib.si.run_scale.rejected`, `stdlib.si.offset.rejected`.
- **dynamic**: bounds are ww_si markers in the type arguments, checked by ww_si-aware clients (CLT-54). 5 cases: `stdlib.si.quantity.bounded.rejected`, `stdlib.si.quantity.bounded.micro.rejected`, `stdlib.si.quantity.bounded.pow2.rejected`, `stdlib.si.quantity.bounded.fullscale.rejected`, `stdlib.si.quantity.bounded.f32.rejected`.
- **dynamic**: by design: the API model has the variants of every build, a host reads it (DER-8). 1 case: `enum.cfg.compiled_out`.
- **dynamic**: head_tail is a wire format of its own without ShrinkWrap serdes, not an API type (SW-46). 1 case: `head_tail.struct`.
- **dynamic**: null is None and Some(None) alike in the corpus value form, as in Python (TOOL-13). 1 case: `option.option.some_none`.
- **dynamic**: the API model has no #[default] values, an old field reads as its type's default (INT-10). 2 cases: `evolution.new_reads_old`, `cfg_attr.new_reads_old`.
- **dynamic**: ww_client_server's Event is a protocol message, not a type of an API in the model; a dynamic consumer reads API types (SW-46 is the same for head_tail). 1 case: `client_server.event.stream_data_path`.
- **dynamic**: ww_client_server's MultiIndex is a protocol message, not a type of an API in the model; a dynamic consumer reads API types (SW-46 is the same for head_tail). 1 case: `client_server.multi_index.list`.
- **dynamic**: ww_client_server's Request is a protocol message, not a type of an API in the model; a dynamic consumer reads API types (SW-46 is the same for head_tail). 4 cases: `client_server.request.absolute_path`, `client_server.request.global_path`, `client_server.request.urgent`, `client_server.request.highest_reserved`.
- **dynamic**: ww_client_server's RequestKind is the request envelope, not a type of an API in the model; a dynamic consumer reads API types (SW-46 is the same for head_tail). 2 cases: `client_server.stream_open.compressed`, `client_server.stream_open.raw`.
- **dynamic**: ww_client_server's StreamOpened is the answer envelope of a stream open, not a type of an API in the model; a dynamic consumer reads API types (SW-46 is the same for head_tail). 2 cases: `client_server.stream_opened.lz4`, `client_server.stream_opened.unknown`.
- **dynamic**: ww_client_server's request envelope, not a type of an API in the model; a dynamic consumer reads API types. 3 cases: `client_server.request.multi_read`, `client_server.request.sink_write`, `client_server.request.sideband_ack`.
- **dynamic**: ww_link's DeviceInfo is a link message, not a type of an API in the model; a dynamic consumer reads API types (SW-46 is the same for head_tail). 3 cases: `device_info.ordinary`, `device_info.no_observe_samples`, `device_info.wire_version`.
- **dynamic**: ww_link's Disconnect payload, not a type of an API in the model; a dynamic consumer reads API types (SW-46 is the same for head_tail). 2 cases: `disconnect_reason.hub_handover`, `disconnect_reason.from_newer`.
- **dynamic**: ww_link's LinkSetup is a link message, not a type of an API in the model; a dynamic consumer reads API types (SW-46 is the same for head_tail). 3 cases: `link_setup.without_meta`, `link_setup.wire_version`, `link_setup.with_meta`.
- **explain**: TailBytes has no form in the API model yet (INT-9). 1 case: `tail_bytes`.
- **explain**: TailSize has no form in the API model yet (INT-9). 1 case: `struct.tail_size`.
- **explain**: a validate function is Rust code the API model can't carry; checks in the model come with sub-type numbers (SW-20). 10 cases: `struct.validate.rejected`, `stdlib.si.dim.rejected`, `stdlib.si.dim.zero.rejected`, `stdlib.si.dim.count.rejected`, `stdlib.si.dim.gaussian.count.rejected`, `stdlib.si.dim.gaussian.ratio.rejected`, `stdlib.si.magnitude.rejected`, `stdlib.si.fraction.rejected`, `stdlib.si.run_scale.rejected`, `stdlib.si.offset.rejected`.
- **explain**: bounds are ww_si markers in the type arguments, checked by ww_si-aware clients (CLT-54). 5 cases: `stdlib.si.quantity.bounded.rejected`, `stdlib.si.quantity.bounded.micro.rejected`, `stdlib.si.quantity.bounded.pow2.rejected`, `stdlib.si.quantity.bounded.fullscale.rejected`, `stdlib.si.quantity.bounded.f32.rejected`.
- **explain**: by design: the API model has the variants of every build, a host reads it (DER-8). 1 case: `enum.cfg.compiled_out`.
- **explain**: head_tail is a wire format of its own without ShrinkWrap serdes, not an API type (SW-46). 1 case: `head_tail.struct`.
- **explain**: null is None and Some(None) alike in the corpus value form, as in Python (TOOL-13). 1 case: `option.option.some_none`.
- **explain**: the API model has no #[default] values, an old field reads as its type's default (INT-10). 2 cases: `evolution.new_reads_old`, `cfg_attr.new_reads_old`.
- **explain**: ww_client_server's Event is a protocol message, not a type of an API in the model. 1 case: `client_server.event.stream_data_path`.
- **explain**: ww_client_server's MultiIndex is a protocol message, not a type of an API in the model. 1 case: `client_server.multi_index.list`.
- **explain**: ww_client_server's Request is a protocol message, not a type of an API in the model. 4 cases: `client_server.request.absolute_path`, `client_server.request.global_path`, `client_server.request.urgent`, `client_server.request.highest_reserved`.
- **explain**: ww_client_server's RequestKind is the request envelope, not a type of an API in the model. 2 cases: `client_server.stream_open.compressed`, `client_server.stream_open.raw`.
- **explain**: ww_client_server's StreamOpened is the answer envelope of a stream open, not a type of an API in the model. 2 cases: `client_server.stream_opened.lz4`, `client_server.stream_opened.unknown`.
- **explain**: ww_client_server's request envelope, not a type of an API in the model. 3 cases: `client_server.request.multi_read`, `client_server.request.sink_write`, `client_server.request.sideband_ack`.
- **explain**: ww_link's DeviceInfo is a link message, not a type of an API in the model. 3 cases: `device_info.ordinary`, `device_info.no_observe_samples`, `device_info.wire_version`.
- **explain**: ww_link's Disconnect payload, not a type of an API in the model. 2 cases: `disconnect_reason.hub_handover`, `disconnect_reason.from_newer`.
- **explain**: ww_link's LinkSetup is a link message, not a type of an API in the model. 3 cases: `link_setup.without_meta`, `link_setup.wire_version`, `link_setup.with_meta`.
- **python**: TailBytes has no form in the API model yet (INT-9). 1 case: `tail_bytes`.
- **python**: TailSize has no form in the API model yet (INT-9). 1 case: `struct.tail_size`.
- **python**: a validate function is Rust code the API model can't carry; checks in the model come with sub-type numbers (SW-20). 10 cases: `struct.validate.rejected`, `stdlib.si.dim.rejected`, `stdlib.si.dim.zero.rejected`, `stdlib.si.dim.count.rejected`, `stdlib.si.dim.gaussian.count.rejected`, `stdlib.si.dim.gaussian.ratio.rejected`, `stdlib.si.magnitude.rejected`, `stdlib.si.fraction.rejected`, `stdlib.si.run_scale.rejected`, `stdlib.si.offset.rejected`.
- **python**: a ww.Quantity holds its unit, not the unit's wire form (Def or Ref), and writes the API's units by reference (CLT-67). 2 cases: `stdlib.si.any_quantity.def`, `stdlib.si.any_quantity.ref`.
- **python**: bounds are ww_si markers in the type arguments, checked by ww_si-aware clients (CLT-54). 5 cases: `stdlib.si.quantity.bounded.rejected`, `stdlib.si.quantity.bounded.micro.rejected`, `stdlib.si.quantity.bounded.pow2.rejected`, `stdlib.si.quantity.bounded.fullscale.rejected`, `stdlib.si.quantity.bounded.f32.rejected`.
- **python**: by design: the API model has the variants of every build (DER-8). 1 case: `enum.cfg.compiled_out`.
- **python**: head_tail is a wire format of its own without ShrinkWrap serdes, not an API type (SW-46). 1 case: `head_tail.struct`.
- **python**: null is None and Some(None) alike in the Python value form (TOOL-13). 1 case: `option.option.some_none`.
- **python**: the API model has no #[default] values (INT-10). 2 cases: `evolution.new_reads_old`, `cfg_attr.new_reads_old`.
- **python**: ww_client_server's Event is a protocol message, not a type of an API in the model. 1 case: `client_server.event.stream_data_path`.
- **python**: ww_client_server's MultiIndex is a protocol message, not a type of an API in the model. 1 case: `client_server.multi_index.list`.
- **python**: ww_client_server's Request is a protocol message, not a type of an API in the model. 4 cases: `client_server.request.absolute_path`, `client_server.request.global_path`, `client_server.request.urgent`, `client_server.request.highest_reserved`.
- **python**: ww_client_server's RequestKind is the request envelope, not a type of an API in the model. 2 cases: `client_server.stream_open.compressed`, `client_server.stream_open.raw`.
- **python**: ww_client_server's StreamOpened is the answer envelope of a stream open, not a type of an API in the model. 2 cases: `client_server.stream_opened.lz4`, `client_server.stream_opened.unknown`.
- **python**: ww_client_server's request envelope, not a type of an API in the model. 3 cases: `client_server.request.multi_read`, `client_server.request.sink_write`, `client_server.request.sideband_ack`.
- **python**: ww_link's DeviceInfo is a link message, not a type of an API in the model. 3 cases: `device_info.ordinary`, `device_info.no_observe_samples`, `device_info.wire_version`.
- **python**: ww_link's Disconnect payload, not a type of an API in the model. 2 cases: `disconnect_reason.hub_handover`, `disconnect_reason.from_newer`.
- **python**: ww_link's LinkSetup is a link message, not a type of an API in the model. 3 cases: `link_setup.without_meta`, `link_setup.wire_version`, `link_setup.with_meta`.

## Wire changes

Accepted by the frozen golden bytes guard (TEST-24) as a deliberate break, each with its EVO item and a `**Breaking:**` CHANGELOG entry:

- `device_info.ordinary`: EVO-25 / the bytes of a device from before the appended user_api_wire, read as None; encoding None now writes its flag.
- `device_info.no_observe_samples`: EVO-25 / the bytes of a device from before the appended user_api_wire, read as None; encoding None now writes its flag.
- `client_server.request.multi_read`: EVO-76 / the priority nibble is now priority: U3 and a reserved bit; NORMAL (2, reserved false) has the old NORMAL's bits, so the bytes stay.
- `client_server.request.sink_write`: EVO-76 / the priority nibble is now priority: U3 and a reserved bit; NORMAL (2, reserved false) has the old NORMAL's bits, so the bytes stay.
- `client_server.request.sideband_ack`: EVO-76 / the priority nibble is now priority: U3 and a reserved bit; NORMAL (2, reserved false) has the old NORMAL's bits, so the bytes stay.
- `client_server.request.absolute_path`: EVO-76 / the priority nibble is now priority: U3 and a reserved bit; NORMAL (2, reserved false) has the old NORMAL's bits, so the bytes stay.
- `client_server.request.global_path`: EVO-76 / the priority nibble is now priority: U3 and a reserved bit; NORMAL (2, reserved false) has the old NORMAL's bits, so the bytes stay.
- `stdlib.si.dim.volt`: EVO-47 / exact_units UNIT-9: a Dim starts with the MarkerId of its system (exact_units::Si here), then as many exponents as the system has.
- `stdlib.si.dim.rad_bit`: EVO-47 / exact_units UNIT-9: a Dim starts with the MarkerId of its system (exact_units::Si here), then as many exponents as the system has.
- `stdlib.si.dim.counted`: EVO-47 / exact_units UNIT-9: a Dim starts with the MarkerId of its system (exact_units::Si here), then as many exponents as the system has.
- `stdlib.si.dim.six_bases`: EVO-47 / exact_units UNIT-9: a Dim starts with the MarkerId of its system (exact_units::Si here), then as many exponents as the system has.
- `stdlib.si.dim.rejected`: EVO-47 / exact_units UNIT-9: a Dim starts with the MarkerId of its system (exact_units::Si here), then as many exponents as the system has.
- `stdlib.si.dim.zero.rejected`: EVO-47 / exact_units UNIT-9: a Dim starts with the MarkerId of its system (exact_units::Si here), then as many exponents as the system has.
- `stdlib.si.dim.count.rejected`: EVO-47 / exact_units UNIT-9: a Dim starts with the MarkerId of its system (exact_units::Si here), then as many exponents as the system has.
- `stdlib.si.dim.root`: EVO-47 / exact_units UNIT-9: a Dim starts with the MarkerId of its system (exact_units::Si here), then as many exponents as the system has.
- `stdlib.si.magnitude.kibi`: EVO-43 / Magnitude's Irrational becomes Symbols with mu0 and g (exact_units NUM-10), and MAX_FACTORS 16 gives 4 more prime slots.
- `stdlib.si.magnitude.duty16`: EVO-55 / exact_units at c132f58: Symbols gains m_u (NUM-32), its count shifting the prime-factor encoding even where symbols is null.
- `stdlib.si.magnitude.degree`: EVO-55 / exact_units at c132f58: Symbols gains m_u (NUM-32); before it, EVO-43 / Magnitude's Irrational becomes Symbols with mu0 and g (exact_units NUM-10), and MAX_FACTORS 16 gives 4 more prime slots.
- `stdlib.si.magnitude.sqrt2`: EVO-43 / Magnitude's Irrational becomes Symbols with mu0 and g (exact_units NUM-10), and MAX_FACTORS 16 gives 4 more prime slots.
- `stdlib.si.magnitude.decibel_in_neper`: EVO-55 / exact_units at c132f58: Symbols gains m_u (NUM-32); before it, EVO-43 / Magnitude's Irrational becomes Symbols with mu0 and g (exact_units NUM-10), and MAX_FACTORS 16 gives 4 more prime slots.
- `stdlib.si.magnitude.pound`: EVO-55 / exact_units at c132f58: Symbols gains m_u (NUM-32); before it, EVO-43 / Magnitude's Irrational becomes Symbols with mu0 and g (exact_units NUM-10), and MAX_FACTORS 16 gives 4 more prime slots.
- `stdlib.si.magnitude.rejected`: EVO-43 / Magnitude's Irrational becomes Symbols with mu0 and g (exact_units NUM-10), and MAX_FACTORS 16 gives 4 more prime slots.
- `stdlib.si.offset.celsius`: EVO-55 / exact_units at c132f58: Symbols gains m_u (NUM-32), its count shifting the prime-factor encoding even where symbols is null.
- `stdlib.si.offset.minus_forty`: EVO-43 / Magnitude's Irrational becomes Symbols with mu0 and g (exact_units NUM-10), and MAX_FACTORS 16 gives 4 more prime slots.
- `stdlib.si.offset.fahrenheit`: EVO-55 / exact_units at c132f58: Symbols gains m_u (NUM-32), its count shifting the prime-factor encoding even where symbols is null.
- `stdlib.si.si_expr.volt`: EVO-47 / exact_units UNIT-9: a Dim starts with the MarkerId of its system (exact_units::Si here), then as many exponents as the system has.
- `stdlib.si.si_expr.pound_force`: EVO-55 / exact_units at c132f58: Symbols gains m_u (NUM-32), its count shifting the prime-factor encoding even where symbols is null; before it, EVO-47 / exact_units UNIT-9: a Dim starts with the MarkerId of its system (exact_units::Si here), then as many exponents as the system has.
- `stdlib.si.si_expr.sqrt`: EVO-47 / exact_units UNIT-9: a Dim starts with the MarkerId of its system (exact_units::Si here), then as many exponents as the system has.
- `stdlib.si.si_expr.decibel_loss_per_hundred_metres`: EVO-55 / exact_units at c132f58: Symbols gains m_u (NUM-32); before it, EVO-47 / exact_units UNIT-9: a Dim starts with the MarkerId of its system (exact_units::Si here), then as many exponents as the system has.
- `stdlib.si.any_quantity.def`: EVO-47 / exact_units UNIT-9: a Dim starts with the MarkerId of its system (exact_units::Si here), then as many exponents as the system has.
- `stdlib.si.si_expr.rejected`: EVO-36 / Ref is variant 1, Sqrt moved up to 5: 6 is the first unknown variant.
- `model.marker_def.unit`: EVO-55 / exact_units at c132f58: Symbols gains m_u (NUM-32); before it, EVO-47 / exact_units UNIT-9: a Dim starts with the MarkerId of its system (exact_units::Si here), then as many exponents as the system has.
- `model.marker_def.unit_conventional`: EVO-55 / exact_units at c132f58: Symbols gains m_u (NUM-32), its count shifting the prime-factor encoding even where symbols is null; before it, EVO-47 / exact_units UNIT-9: a Dim starts with the MarkerId of its system (exact_units::Si here), then as many exponents as the system has.
- `model.marker_def.unit_kind`: EVO-47 / exact_units UNIT-9: a Dim starts with the MarkerId of its system (exact_units::Si here), then as many exponents as the system has.
- `model.marker_def.base`: EVO-47 / exact_units UNIT-9: a Dim starts with the MarkerId of its system (exact_units::Si here), then as many exponents as the system has.
- `model.marker_def.scale`: EVO-55 / exact_units at c132f58: Symbols gains m_u (NUM-32), its count shifting the prime-factor encoding even where symbols is null; before it, EVO-43 / Magnitude's Irrational becomes Symbols with mu0 and g (exact_units NUM-10), and MAX_FACTORS 16 gives 4 more prime slots; before it, EVO-35 / a unit's definition is exact_units' UnitDef with its identity (crate and marker); origins and scales carry ids (UNIT-7, STD-106).
- `model.marker_def.product`: EVO-35 / a unit's definition is exact_units' UnitDef with its identity (crate and marker); origins and scales carry ids (UNIT-7, STD-106).
- `model.marker_def.quotient`: EVO-35 / a unit's definition is exact_units' UnitDef with its identity (crate and marker); origins and scales carry ids (UNIT-7, STD-106).
- `model.marker_def.origin`: EVO-55 / exact_units at c132f58: Symbols gains m_u (NUM-32), its count shifting the prime-factor encoding even where symbols is null; before it, EVO-48 / exact_units PT-4: an origin carries its reference whole (id, name and how its zero is known) instead of its MarkerId.
- `model.marker_def.codec`: EVO-55 / exact_units at c132f58: CodecDef gains codes and nominal, appended after spare, and curve after nominal.
- `model.marker_def.codec.spare`: EVO-55 / exact_units at c132f58: CodecDef gains codes and nominal, appended after spare, and curve after nominal.
- `model.marker_def.codec.curve`: EVO-53 / exact_units LVL-8/LVL-9: CodecDef.curve, a CurveDef above a codec.
- `model.marker_def.maybe_nan`: EVO-35 / a unit's definition is exact_units' UnitDef with its identity (crate and marker); origins and scales carry ids (UNIT-7, STD-106).
- `model.marker_def.quantity`: EVO-47 / exact_units UNIT-9: a Dim starts with the MarkerId of its system (exact_units::Si here), then as many exponents as the system has.
- `model.marker_def.quantity_kind`: EVO-47 / exact_units UNIT-9: a Dim starts with the MarkerId of its system (exact_units::Si here), then as many exponents as the system has.
- `model.item_struct_extra.marker`: EVO-55 / exact_units at c132f58: Symbols gains m_u (NUM-32); before it, EVO-47 / exact_units UNIT-9: a Dim starts with the MarkerId of its system (exact_units::Si here), then as many exponents as the system has.
