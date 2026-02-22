#[derive(Copy, Clone, Debug, Default)]
pub enum Repr {
    U(u8),
    #[default]
    UNib32,
}

impl Repr {
    pub fn parse_str(s: &str) -> Option<Self> {
        if s == "unib32" {
            return Some(Repr::UNib32);
        }
        let s = s.strip_prefix("u")?;
        let bits: u8 = s.parse().ok()?;
        Some(Repr::U(bits))
    }

    pub fn max_discriminant(&self) -> u32 {
        match self {
            Repr::U(bits) => {
                if *bits == 32 {
                    u32::MAX
                } else {
                    2u32.pow(*bits as u32) - 1
                }
            }
            Repr::UNib32 => u32::MAX,
        }
    }

    pub fn required_bits(&self) -> u8 {
        match self {
            Repr::U(bits) => *bits,
            Repr::UNib32 => 32,
        }
    }

    pub fn std_bits(&self) -> u8 {
        match &self {
            Repr::U(4) => 8,
            Repr::U(8) => 8,
            Repr::U(16) => 16,
            Repr::U(32) => 32,
            Repr::UNib32 => 32,
            Repr::U(bits) if *bits < 8 => 8,
            Repr::U(bits) if *bits < 16 => 16,
            Repr::U(bits) if *bits < 32 => 32,
            u => unimplemented!("discriminant_type {:?}", u),
        }
    }
}
