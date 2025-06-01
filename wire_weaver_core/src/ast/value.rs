use proc_macro2::Literal;

#[derive(Clone, Debug)]
pub enum Value {
    Bool(bool),
    F32(f32),
    F64(f64),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    U128(u128),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    I128(i128),
}

impl Value {
    pub fn to_lit(&self) -> Literal {
        match self {
            // Value::Bool(_) => {}
            Value::F32(val) => Literal::f32_suffixed(*val),
            u => unimplemented!("{u:?}"), // Value::F64(_) => {}
                                          // Value::U8(_) => {}
                                          // Value::U16(_) => {}
                                          // Value::U32(_) => {}
                                          // Value::U64(_) => {}
                                          // Value::U128(_) => {}
                                          // Value::I8(_) => {}
                                          // Value::I16(_) => {}
                                          // Value::I32(_) => {}
                                          // Value::I64(_) => {}
                                          // Value::I128(_) => {}
        }
    }
}
