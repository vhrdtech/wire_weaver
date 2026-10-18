//! 64 rows of a buck stage (48 V in, 12 V out, 8 A) sampled once per switching period, for the high-rate observe
//! bench ([super::high_rate]). Synthetic, not simulated: the inductor current's period average with a ±40 mA
//! control ripple (a triangle of 20 periods) and ±10 mA of ADC noise, the output voltage with ±10 mV of ripple and
//! ±6 mV of noise, the input with ±30 mV of noise; at row 32 the load steps from 8 A to 12 A, the current slews in
//! 200 mA per period and the output dips by 150 mV and recovers. So the first batch of 32 is quiet and the second a
//! transient, as in [super::foc_signals]. The noise is a fixed LCG, the same on every build.

/// Rows in the tables.
pub const ROWS: usize = 64;

/// Uniform noise in `-a..=a` for row `i` and channel `c`, from a fixed LCG.
const fn noise(i: usize, c: u32, a: i32) -> i32 {
    let mut x = (i as u32).wrapping_mul(2_654_435_761) ^ c.wrapping_mul(0x9E37_79B9);
    x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    x ^= x >> 16;
    (x % (2 * a as u32 + 1)) as i32 - a
}

/// A triangle of period 20 rows between `-a` and `a`.
const fn ripple(i: usize, a: i32) -> i32 {
    let p = (i % 20) as i32;
    let up = if p < 10 { p } else { 20 - p };
    (up * 2 * a) / 10 - a
}

/// The load current in mA: 8 A, then 12 A from row 32.
const fn load(i: usize) -> i32 {
    if i < 32 { 8000 } else { 12_000 }
}

/// The table of `f` over every row (a const fn can't call a fn pointer).
macro_rules! table {
    ($f:ident) => {{
        let mut t = [0; ROWS];
        let mut i = 0;
        while i < ROWS {
            t[i] = $f(i);
            i += 1;
        }
        t
    }};
}

const fn i_l(i: usize) -> i32 {
    // slews towards the load at 200 mA per period
    let slewed = if i < 32 {
        8000
    } else {
        let s = 8000 + 200 * (i as i32 - 31);
        if s > load(i) { load(i) } else { s }
    };
    slewed + ripple(i, 40) + noise(i, 1, 10)
}

const fn v_out(i: usize) -> i32 {
    // dips by 150 mV at the step, recovers in 20 periods
    let dip = if i >= 32 && i < 52 {
        -150 + 150 * (i as i32 - 32) / 20
    } else {
        0
    };
    12_000 + dip + ripple(i, 10) + noise(i, 2, 6)
}

const fn v_in(i: usize) -> i32 {
    48_000 + noise(i, 3, 30)
}

/// Inductor current, mA.
pub const I_L: [i32; ROWS] = table!(i_l);
/// Output voltage, mV.
pub const V_OUT: [i32; ROWS] = table!(v_out);
/// Input voltage, mV.
pub const V_IN: [i32; ROWS] = table!(v_in);
