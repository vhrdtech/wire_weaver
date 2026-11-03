//! APIs of the flash budget of a server (TEST-30, `examples_mcu/fw_budget --flash`, `just flash-budget`).
//!
//! Each root is built into its own server bin in `examples_mcu/mcu_qemu` (`qemu-flash-*`), and the budget is the
//! difference between their sizes:
//!
//! - [Floor]: one method, what any server costs.
//! - [Base]: two resources of each kind, at a level of its own; [BaseOnly] is just that level.
//! - `Calls8` / `Calls16`, `CallsArgs8` / `CallsArgs16`, `Props8` / `Props16`, `Streams8` / `Streams16`: [Base]
//!   at `base` plus 8 or 16 resources of one kind at the root, so `(size(*16) - size(*8)) / 8` is the cost of one
//!   more resource of that kind. Both have resources at the root, so the level's own code is the same in both and
//!   only the arms differ (against [BaseOnly], whose root holds only a level, the root's code changes shape too).
//! - `Typed8` / `Typed16`: calls with an argument and an output of their own types (`InN`, `OutN`), as in a real
//!   API, so their serdes is paid per resource too.
//!
//! Every resource of the first four kinds uses the same types, so their serdes is paid once, in [BaseOnly]: the
//! differences are the dispatcher's own bytes per resource. `Typed*` adds each resource's serdes.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

/// Resources of one kind the larger of a pair of roots has more than the smaller one.
pub const EXTRA: u32 = 8;

/// An argument and a property value.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct Code(pub u16);

/// A call's output and a stream item.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct Reading(pub u32);

/// What any server costs: one method.
#[ww_api_root]
pub trait Floor {
    fn ping();
}

/// Two resources of each kind.
#[ww_trait]
pub trait Base {
    fn c0();
    fn c1();
    fn a0(code: Code) -> Reading;
    fn a1(code: Code) -> Reading;
    property!(rw p0: Code);
    property!(rw p1: Code);
    stream!(s0: Reading);
    stream!(s1: Reading);
}

/// [Base] alone, the reference of the `More*` roots.
#[ww_api_root]
pub trait BaseOnly {
    ww_impl!(base: Base);
}

/// [Base] and 8 calls without arguments or output.
#[ww_api_root]
pub trait Calls8 {
    ww_impl!(base: Base);
    fn x0();
    fn x1();
    fn x2();
    fn x3();
    fn x4();
    fn x5();
    fn x6();
    fn x7();
}

/// [Base] and 16 calls without arguments or output.
#[ww_api_root]
pub trait Calls16 {
    ww_impl!(base: Base);
    fn x0();
    fn x1();
    fn x2();
    fn x3();
    fn x4();
    fn x5();
    fn x6();
    fn x7();
    fn x8();
    fn x9();
    fn x10();
    fn x11();
    fn x12();
    fn x13();
    fn x14();
    fn x15();
}

/// [Base] and 8 calls with an argument and an output.
#[ww_api_root]
pub trait CallsArgs8 {
    ww_impl!(base: Base);
    fn x0(code: Code) -> Reading;
    fn x1(code: Code) -> Reading;
    fn x2(code: Code) -> Reading;
    fn x3(code: Code) -> Reading;
    fn x4(code: Code) -> Reading;
    fn x5(code: Code) -> Reading;
    fn x6(code: Code) -> Reading;
    fn x7(code: Code) -> Reading;
}

/// [Base] and 16 calls with an argument and an output.
#[ww_api_root]
pub trait CallsArgs16 {
    ww_impl!(base: Base);
    fn x0(code: Code) -> Reading;
    fn x1(code: Code) -> Reading;
    fn x2(code: Code) -> Reading;
    fn x3(code: Code) -> Reading;
    fn x4(code: Code) -> Reading;
    fn x5(code: Code) -> Reading;
    fn x6(code: Code) -> Reading;
    fn x7(code: Code) -> Reading;
    fn x8(code: Code) -> Reading;
    fn x9(code: Code) -> Reading;
    fn x10(code: Code) -> Reading;
    fn x11(code: Code) -> Reading;
    fn x12(code: Code) -> Reading;
    fn x13(code: Code) -> Reading;
    fn x14(code: Code) -> Reading;
    fn x15(code: Code) -> Reading;
}

/// [Base] and 8 read-write properties.
#[ww_api_root]
pub trait Props8 {
    ww_impl!(base: Base);
    property!(rw x0: Code);
    property!(rw x1: Code);
    property!(rw x2: Code);
    property!(rw x3: Code);
    property!(rw x4: Code);
    property!(rw x5: Code);
    property!(rw x6: Code);
    property!(rw x7: Code);
}

/// [Base] and 16 read-write properties.
#[ww_api_root]
pub trait Props16 {
    ww_impl!(base: Base);
    property!(rw x0: Code);
    property!(rw x1: Code);
    property!(rw x2: Code);
    property!(rw x3: Code);
    property!(rw x4: Code);
    property!(rw x5: Code);
    property!(rw x6: Code);
    property!(rw x7: Code);
    property!(rw x8: Code);
    property!(rw x9: Code);
    property!(rw x10: Code);
    property!(rw x11: Code);
    property!(rw x12: Code);
    property!(rw x13: Code);
    property!(rw x14: Code);
    property!(rw x15: Code);
}

/// [Base] and 8 streams.
#[ww_api_root]
pub trait Streams8 {
    ww_impl!(base: Base);
    stream!(x0: Reading);
    stream!(x1: Reading);
    stream!(x2: Reading);
    stream!(x3: Reading);
    stream!(x4: Reading);
    stream!(x5: Reading);
    stream!(x6: Reading);
    stream!(x7: Reading);
}

/// [Base] and 16 streams.
#[ww_api_root]
pub trait Streams16 {
    ww_impl!(base: Base);
    stream!(x0: Reading);
    stream!(x1: Reading);
    stream!(x2: Reading);
    stream!(x3: Reading);
    stream!(x4: Reading);
    stream!(x5: Reading);
    stream!(x6: Reading);
    stream!(x7: Reading);
    stream!(x8: Reading);
    stream!(x9: Reading);
    stream!(x10: Reading);
    stream!(x11: Reading);
    stream!(x12: Reading);
    stream!(x13: Reading);
    stream!(x14: Reading);
    stream!(x15: Reading);
}

/// [Base] and 8 calls with arguments and outputs of their own types.
#[ww_api_root]
pub trait Typed8 {
    ww_impl!(base: Base);
    fn x0(arg: In0) -> Out0;
    fn x1(arg: In1) -> Out1;
    fn x2(arg: In2) -> Out2;
    fn x3(arg: In3) -> Out3;
    fn x4(arg: In4) -> Out4;
    fn x5(arg: In5) -> Out5;
    fn x6(arg: In6) -> Out6;
    fn x7(arg: In7) -> Out7;
}

/// [Base] and 16 calls with arguments and outputs of their own types.
#[ww_api_root]
pub trait Typed16 {
    ww_impl!(base: Base);
    fn x0(arg: In0) -> Out0;
    fn x1(arg: In1) -> Out1;
    fn x2(arg: In2) -> Out2;
    fn x3(arg: In3) -> Out3;
    fn x4(arg: In4) -> Out4;
    fn x5(arg: In5) -> Out5;
    fn x6(arg: In6) -> Out6;
    fn x7(arg: In7) -> Out7;
    fn x8(arg: In8) -> Out8;
    fn x9(arg: In9) -> Out9;
    fn x10(arg: In10) -> Out10;
    fn x11(arg: In11) -> Out11;
    fn x12(arg: In12) -> Out12;
    fn x13(arg: In13) -> Out13;
    fn x14(arg: In14) -> Out14;
    fn x15(arg: In15) -> Out15;
}

/// The argument of `x0` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct In0 {
    pub code: Code,
    pub flag: bool,
}

/// The output of `x0` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct Out0 {
    pub reading: Reading,
    pub code: Option<Code>,
}

/// The argument of `x1` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct In1 {
    pub code: Code,
    pub flag: bool,
}

/// The output of `x1` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct Out1 {
    pub reading: Reading,
    pub code: Option<Code>,
}

/// The argument of `x2` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct In2 {
    pub code: Code,
    pub flag: bool,
}

/// The output of `x2` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct Out2 {
    pub reading: Reading,
    pub code: Option<Code>,
}

/// The argument of `x3` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct In3 {
    pub code: Code,
    pub flag: bool,
}

/// The output of `x3` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct Out3 {
    pub reading: Reading,
    pub code: Option<Code>,
}

/// The argument of `x4` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct In4 {
    pub code: Code,
    pub flag: bool,
}

/// The output of `x4` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct Out4 {
    pub reading: Reading,
    pub code: Option<Code>,
}

/// The argument of `x5` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct In5 {
    pub code: Code,
    pub flag: bool,
}

/// The output of `x5` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct Out5 {
    pub reading: Reading,
    pub code: Option<Code>,
}

/// The argument of `x6` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct In6 {
    pub code: Code,
    pub flag: bool,
}

/// The output of `x6` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct Out6 {
    pub reading: Reading,
    pub code: Option<Code>,
}

/// The argument of `x7` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct In7 {
    pub code: Code,
    pub flag: bool,
}

/// The output of `x7` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct Out7 {
    pub reading: Reading,
    pub code: Option<Code>,
}

/// The argument of `x8` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct In8 {
    pub code: Code,
    pub flag: bool,
}

/// The output of `x8` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct Out8 {
    pub reading: Reading,
    pub code: Option<Code>,
}

/// The argument of `x9` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct In9 {
    pub code: Code,
    pub flag: bool,
}

/// The output of `x9` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct Out9 {
    pub reading: Reading,
    pub code: Option<Code>,
}

/// The argument of `x10` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct In10 {
    pub code: Code,
    pub flag: bool,
}

/// The output of `x10` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct Out10 {
    pub reading: Reading,
    pub code: Option<Code>,
}

/// The argument of `x11` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct In11 {
    pub code: Code,
    pub flag: bool,
}

/// The output of `x11` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct Out11 {
    pub reading: Reading,
    pub code: Option<Code>,
}

/// The argument of `x12` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct In12 {
    pub code: Code,
    pub flag: bool,
}

/// The output of `x12` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct Out12 {
    pub reading: Reading,
    pub code: Option<Code>,
}

/// The argument of `x13` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct In13 {
    pub code: Code,
    pub flag: bool,
}

/// The output of `x13` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct Out13 {
    pub reading: Reading,
    pub code: Option<Code>,
}

/// The argument of `x14` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct In14 {
    pub code: Code,
    pub flag: bool,
}

/// The output of `x14` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct Out14 {
    pub reading: Reading,
    pub code: Option<Code>,
}

/// The argument of `x15` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct In15 {
    pub code: Code,
    pub flag: bool,
}

/// The output of `x15` of the `Typed*` roots.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct Out15 {
    pub reading: Reading,
    pub code: Option<Code>,
}
