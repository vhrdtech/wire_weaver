//! The types the cases serialize. The `busgen` ones follow busgen_proto (fpga_tools, BUS-1): the request and
//! reply enums of a small softcore firmware, here with shrink_wrap derives instead of its hand-written codec.

use shrink_wrap::prelude::*;

// `$sized` and `$fin` are the size assumption directives of the types that can have one: nothing for the
// evolvable default, `, sized` / `, final_structure` with the `final` feature.
macro_rules! types {
    ([$($sized:tt)*], [$($fin:tt)*]) => {
        /// One line's waveform: a plain struct of integers and flags
        #[derive_shrink_wrap(borrowed, derive(Clone, Copy) $($sized)*)]
        pub struct Wave {
            pub line: u8,
            pub idle: bool,
            pub hold: bool,
            pub xor: bool,
            pub delay: u32,
            pub high: u32,
            pub low: u32,
            pub n: u32,
        }

        #[derive_shrink_wrap(borrowed, derive(Clone, Copy) $($sized)*)]
        pub struct UartFormat {
            pub bit: u32,
            pub bits: u8,
            pub parity: u8,
            pub stop: u32,
            pub msb_first: bool,
            pub inv: bool,
        }

        #[derive_shrink_wrap(borrowed, derive(Clone, Copy) $($sized)*)]
        pub struct I2cTiming {
            pub hd: u32,
            pub lo: u32,
            pub hi: u32,
            pub su: u32,
            pub buf: u32,
            pub unit: u32,
        }

        /// A struct with two `Option`s
        #[derive_shrink_wrap(borrowed, derive(Clone, Copy) $($fin)*)]
        pub struct I2cSlave {
            pub on: bool,
            pub scl: u8,
            pub sda: u8,
            pub log: u8,
            pub stretch: u8,
            pub stretch_c: u32,
            pub filt: u8,
            pub nack_at: u16,
            pub a10: Option<u16>,
            pub ptr: Option<u8>,
        }

        /// A struct with one list
        #[derive_shrink_wrap(derive(Clone, Copy) $($fin)*)]
        pub struct Load<'i> {
            pub off: u16,
            pub frames: RefVec<'i, u16>,
        }

        /// An enum of small variants, no lists
        #[derive_shrink_wrap(borrowed, derive(Clone, Copy), ww_repr = u8 $($fin)*)]
        pub enum Small {
            Hello,
            Stop { mask: u16 },
            Mark { on: bool, width: u32, lead: u32, line: u8 },
            Pull { line: u8, up: u8, down: u8 },
            Wait { timeout_ms: u32 },
        }

        #[derive_shrink_wrap(derive(Clone, Copy) $($fin)*)]
        pub struct Utx<'i> {
            pub ch: u8,
            pub line: u8,
            pub fmt: UartFormat,
            pub gap: u32,
            pub delay: u32,
            pub off: u16,
            pub n: u16,
            pub brklen: u32,
            pub trunc: u8,
            pub glitch: Option<Wave>,
            pub perr: RefVec<'i, u16>,
            pub ferr: RefVec<'i, u16>,
            pub brk: RefVec<'i, u16>,
            pub defer: bool,
        }

        #[derive_shrink_wrap(derive(Clone, Copy) $($fin)*)]
        pub struct I2c<'i> {
            pub scl: u8,
            pub sda: u8,
            pub log: bool,
            pub t: I2cTiming,
            pub delay: u32,
            pub off: u16,
            pub n: u16,
            pub glitches: RefVec<'i, Wave>,
            pub pp: u8,
            pub keep_log: bool,
            pub defer: bool,
        }

        /// busgen_proto's `Request` (host to board)
        #[derive_shrink_wrap(derive(Clone, Copy), ww_repr = u8 $($fin)*)]
        pub enum Request<'i> {
            Hello,
            Status,
            Pins,
            Reset,
            Stop { mask: u16 },
            Release { lines: u8 },
            Mark { on: bool, width: u32, lead: u32, line: u8 },
            Pull { line: u8, up: u8, down: u8 },
            Waves { waves: RefVec<'i, Wave>, defer: bool },
            Go,
            Wait { timeout_ms: u32 },
            Load { off: u16, frames: RefVec<'i, u16> },
            Dump { off: u16, n: u16 },
            Utx(Utx<'i>),
            Urx { line: u8, fmt: UartFormat, defer: bool },
            UrxGet { max: u16, flush: bool },
            I2c(I2c<'i>),
            I2cSlave { s: I2cSlave, keep_log: bool, defer: bool },
        }

        /// A received UART frame: its bits and flags, and when it started
        #[derive_shrink_wrap(borrowed, derive(Clone, Copy) $($sized)*)]
        pub struct RxFrame {
            pub d: u16,
            pub t: u32,
        }

        /// busgen_proto's `Reply` (board to host)
        #[derive_shrink_wrap(derive(Clone, Copy), ww_repr = u8 $($fin)*)]
        pub enum Reply<'i> {
            Hello {
                proto: u16,
                fw: &'i str,
                clk_hz: u32,
                lines: u8,
                mark_line: u8,
                txbuf: u16,
                rxbuf: u16,
                rx_sync: u8,
                family: u8,
            },
            Ok,
            Run { run: u32, started: bool, mark: u32, lead: u32, mark_line: u8 },
            Status {
                run: u32,
                busy: u16,
                t: u32,
                oe: u8,
                out: u8,
                mark_on: bool,
                mark: u32,
                lead: u32,
                pull_oe: u8,
                pull_out: u8,
                deferred: u16,
                mark_line: u8,
            },
            Pins { input: u8, oe: u8, out: u8 },
            Wait { run: u32, end: u32 },
            Dump { off: u16, frames: RefVec<'i, u16> },
            UrxGet { frames: RefVec<'i, RxFrame>, more: u16, overflow: u16 },
            Err { code: u8, arg: u16 },
        }
    };
}

#[cfg(not(feature = "final"))]
types!([], []);
#[cfg(feature = "final")]
types!([, sized], [, final_structure]);
