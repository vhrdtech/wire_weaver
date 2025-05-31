use crate::ast::ident::Ident;
use crate::ast::{Item, Type};
use shrink_wrap::BufWriter;
use std::collections::HashMap;
use syn::{Expr, ExprStruct, Lit, Member};

// TODO: add explanation of which item is in which byte
pub fn ser_literal(lit: &str, item: &Item) -> Result<Vec<u8>, ()> {
    let lit: ExprStruct = syn::parse_str(lit).unwrap();
    // println!("{lit:?}");
    let fields = lit
        .fields
        .into_iter()
        .filter_map(|f| {
            if let Member::Named(ident) = f.member {
                Some((Ident::from(ident), f.expr))
            } else {
                None
            }
        })
        .collect::<HashMap<_, _>>();
    println!("{fields:?}");
    // TODO: check for missing, repeating and unknown fields

    let mut buf = [0u8; 128];
    let mut wr = BufWriter::new(&mut buf);
    match item {
        Item::Struct(item_struct) => {
            for field in &item_struct.fields {
                let expr = fields.get(&field.ident).unwrap();
                match field.ty {
                    Type::Bool => {
                        let Expr::Lit(expr_lit) = expr else {
                            panic!("expected Lit")
                        };
                        let Lit::Bool(lit_bool) = &expr_lit.lit else {
                            panic!("expected LitBool")
                        };
                        wr.write_bool(lit_bool.value).unwrap();
                    }
                    Type::U8 => {
                        let Expr::Lit(expr_lit) = expr else {
                            panic!("expected Lit")
                        };
                        let Lit::Int(lit_int) = &expr_lit.lit else {
                            panic!("expected LitInt")
                        };
                        let value: u8 = lit_int.base10_digits().parse().unwrap();
                        wr.write_u8(value).unwrap();
                    }
                    _ => unimplemented!(),
                }
            }
        }
        Item::Enum(_) => unimplemented!(),
        Item::Const(_) => unimplemented!(),
    }
    Ok(wr.finish().unwrap().to_vec())
}

#[cfg(test)]
mod tests {
    use crate::ast::ident::Ident;
    use crate::ast::{Docs, Field, Item, ItemStruct, Type};
    use crate::eval::ser_literal;

    #[test]
    fn simple_struct() {
        let struct_def = ItemStruct {
            docs: Docs::empty(),
            derive: vec![],
            is_final: false,
            ident: Ident::new("MyStruct"),
            fields: vec![Field::new(0, "a", Type::Bool), Field::new(1, "b", Type::U8)],
        };
        let buf = ser_literal("MyStruct { a: true, b: 10 }", &Item::Struct(struct_def)).unwrap();
        assert_eq!(buf, &[0b1000_0000, 10]);
    }
}
