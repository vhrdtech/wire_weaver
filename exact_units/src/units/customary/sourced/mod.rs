//! Imperial and US customary units (STD-72), the sourced catalogue: every unit as the exact rational of SI its legal
//! definition gives, with the source of that definition, the value NIST SP 811 prints for it, and the groups of
//! names that are ambiguous on their own (a bare "gallon", "ton" or "BTU" never names a unit here).
//!
//! Test-only: the data the markers of [customary](super) are checked against (`markers`), with the citations, the
//! values SP 811 prints and the ambiguity groups the docs table is made of. The source extracts are kept with the
//! STD-72 design notes.

mod catalogue;
mod mag;
mod markers;
mod sources;
mod tests;

pub use catalogue::{AMBIGUOUS, ORIGINS, UNITS};
pub use mag::Mag;
pub use sources::Src;

/// Exponents of s, m, kg, A, K, mol, cd, rad (si-in's order; angle is a base, D15).
pub type Dim = [i8; 8];

pub const fn dim(s: i8, m: i8, kg: i8, k: i8, rad: i8) -> Dim {
    [s, m, kg, 0, k, 0, 0, rad]
}

/// Where a definition or a value is stated: the document and the place in it.
#[derive(Clone, Copy, Debug)]
pub struct Cite(pub Src, pub &'static str);

/// A value as NIST SP 811 Appendix B.8 prints it: mantissa text and power of ten, bold (exact) or rounded.
#[derive(Clone, Copy, Debug)]
pub struct Printed {
    pub mantissa: &'static str,
    pub exp10: i16,
    pub bold: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exact {
    /// Fixed by definition: the magnitude is the exact value.
    Defined,
    /// A physical property (water or mercury at a temperature, a calorie at a temperature): the magnitude is the
    /// value SP 811 prints, not a definition, and converts only with that rounding stated.
    Measured,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Current,
    /// Kept for old data, with why.
    Legacy(&'static str),
}

/// One unit: `1 <unit> = mag · (coherent SI unit of dim)`; radians for angles, kelvin for temperature intervals.
#[derive(Clone, Copy, Debug)]
pub struct Unit {
    /// Unique, never a bare ambiguous name.
    pub id: &'static str,
    pub name: &'static str,
    /// A proposal, unique across the table; STD-73 decides the display and parse symbols.
    pub symbol: &'static str,
    pub dim: Dim,
    pub mag: Mag,
    /// The defining relation as the source states it.
    pub def: &'static str,
    pub exact: Exact,
    pub sources: &'static [Cite],
    pub sp811: Option<Printed>,
    pub status: Status,
}

impl Unit {
    pub const fn new(
        id: &'static str,
        name: &'static str,
        symbol: &'static str,
        dim: Dim,
        mag: Mag,
        def: &'static str,
        sources: &'static [Cite],
    ) -> Unit {
        Unit {
            id,
            name,
            symbol,
            dim,
            mag,
            def,
            exact: Exact::Defined,
            sources,
            sp811: None,
            status: Status::Current,
        }
    }

    /// SP 811 B.8 prints `mantissa E exp10`, rounded.
    pub const fn sp(mut self, mantissa: &'static str, exp10: i16) -> Unit {
        self.sp811 = Some(Printed {
            mantissa,
            exp10,
            bold: false,
        });
        self
    }

    /// SP 811 B.8 prints `mantissa E exp10` in bold: exact.
    pub const fn bold(mut self, mantissa: &'static str, exp10: i16) -> Unit {
        self.sp811 = Some(Printed {
            mantissa,
            exp10,
            bold: true,
        });
        self
    }

    pub const fn measured(mut self) -> Unit {
        self.exact = Exact::Measured;
        self
    }

    pub const fn legacy(mut self, why: &'static str) -> Unit {
        self.status = Status::Legacy(why);
        self
    }

    /// No fetched source states this unit's definition (the reason is in its `Unsourced` cite).
    pub fn unsourced(&self) -> bool {
        self.sources.iter().any(|c| c.0 == Src::Unsourced)
    }
}

/// The zero of an absolute temperature scale whose interval is the unit `unit`: `T/K = (t/unit) · mag + zero`.
#[derive(Clone, Copy, Debug)]
pub struct Origin {
    pub id: &'static str,
    pub unit: &'static str,
    /// The scale's zero above 0 K, in kelvin (`None`: at 0 K).
    pub zero: Option<Mag>,
    pub sources: &'static [Cite],
}

/// A name that means different units: it is never an id or a symbol, only a pointer to the qualified ones.
#[derive(Clone, Copy, Debug)]
pub struct Ambiguous {
    pub bare: &'static str,
    /// Symbols (and abbreviations) used for more than one member.
    pub symbols: &'static [&'static str],
    pub members: &'static [&'static str],
    pub note: &'static str,
}
