//! The documents the catalogue cites, each read in full on 27 Oct 2026 (copies kept with the STD-72 design notes).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Src {
    /// The 1959 international yard and pound: 1 yd = 0.9144 m, 1 lb = 0.45359237 kg, US survey foot 1200/3937 m.
    Yd1959,
    /// NIST SP 811 (2008 ed., web version updated 2025) Appendix B (B.6 survey foot, B.8 factors, footnotes).
    Sp811,
    /// NIST Handbook 44 (2026) Appendix C, General Tables of Units of Measurement.
    Hb44,
    /// UK Weights and Measures Act 1985, Schedule 1 (as in force 25 Oct 2026).
    Wma1985,
    /// 3rd CGPM (1901) Resolution 2: standard acceleration of gravity g_n = 980.665 cm/s².
    Cgpm1901,
    /// BIPM, The International System of Units, 8th edition (2006), Tables 6 and 8.
    SiBrochure8,
    /// 85 FR 62698 (5 Oct 2020): the US survey foot deprecated after 31 Dec 2022.
    Fr2020,
    /// Council Directive 80/181/EEC (units of measurement), Annex, as on legislation.gov.uk.
    Eec80181,
    /// 21 CFR 101.9(b)(5)(viii): household measures for US nutrition labeling.
    Cfr21_101_9,
    /// No fetched primary source states this definition; the `at` text says what is missing.
    Unsourced,
}

impl Src {
    /// Every document cited (all but `Unsourced`), in the order of the docs page.
    pub const CITED: [Src; 9] = [
        Src::Yd1959,
        Src::Sp811,
        Src::Hb44,
        Src::Wma1985,
        Src::Cgpm1901,
        Src::SiBrochure8,
        Src::Fr2020,
        Src::Eec80181,
        Src::Cfr21_101_9,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Src::Yd1959 => {
                "Refinement of values for the yard and the pound, Federal Register 24 FR 5348 (1 Jul 1959), reprinted in NBS Misc. Publ. 233 (1960)"
            }
            Src::Sp811 => {
                "NIST Special Publication 811, Guide for the Use of the International System of Units (SI), Appendix B"
            }
            Src::Hb44 => {
                "NIST Handbook 44 (2026), Appendix C, General Tables of Units of Measurement"
            }
            Src::Wma1985 => "Weights and Measures Act 1985 (c. 72), Schedule 1",
            Src::Cgpm1901 => "3rd CGPM (1901), Resolution 2",
            Src::SiBrochure8 => "BIPM, The International System of Units (SI), 8th edition (2006)",
            Src::Fr2020 => {
                "Federal Register 85 FR 62698 (5 Oct 2020), deprecation of the U.S. survey foot"
            }
            Src::Eec80181 => {
                "Council Directive 80/181/EEC of 20 December 1979 on units of measurement"
            }
            Src::Cfr21_101_9 => "21 CFR 101.9 Nutrition labeling of food",
            Src::Unsourced => "unsourced",
        }
    }

    pub fn url(self) -> &'static str {
        match self {
            Src::Yd1959 => "https://nvlpubs.nist.gov/nistpubs/Legacy/MP/nbsmiscellaneouspub233.pdf",
            Src::Sp811 => {
                "https://www.nist.gov/pml/special-publication-811/nist-guide-si-appendix-b-conversion-factors/nist-guide-si-appendix-b8"
            }
            Src::Hb44 => "https://www.nist.gov/document/2026-nist-handbook-44-appendix-c",
            Src::Wma1985 => "https://www.legislation.gov.uk/ukpga/1985/72/schedule/1",
            Src::Cgpm1901 => "https://www.bipm.org/en/committees/cg/cgpm/3-1901/resolution-2",
            Src::SiBrochure8 => "https://www.bipm.org/documents/20126/41483022/si_brochure_8.pdf",
            Src::Fr2020 => "https://www.govinfo.gov/content/pkg/FR-2020-10-05/pdf/2020-21902.pdf",
            Src::Eec80181 => "https://www.legislation.gov.uk/eudr/1980/181/annex",
            Src::Cfr21_101_9 => "https://www.ecfr.gov/current/title-21/section-101.9",
            Src::Unsourced => "",
        }
    }
}
