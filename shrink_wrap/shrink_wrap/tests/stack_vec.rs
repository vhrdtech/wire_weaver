use hex_literal::hex;
use shrink_wrap::prelude::*;

#[derive_shrink_wrap]
struct DynamicThings<'i> {
    a: RefVec<'i, u8>,
    b: &'i str,
    c: RefVec<'i, MoreDynamic<'i>>,
}

#[derive_shrink_wrap]
#[derive(Clone)]
struct MoreDynamic<'i> {
    d: RefVec<'i, u8>,
}

#[test]
fn stack_vec() {
    let obj = DynamicThings {
        a: RefVec::new_bytes(&[1, 2, 3, 4, 5]),
        b: "abc",
        c: RefVec::Slice {
            slice: &[
                MoreDynamic {
                    d: RefVec::new_bytes(&[6, 7, 8]),
                },
                MoreDynamic {
                    d: RefVec::new_bytes(&[9, 10, 11]),
                },
            ],
        },
    };
    let mut on_stack = StackVec::<32, _>::some(obj).unwrap();
    assert_eq!(
        on_stack.bytes(),
        hex!("0102030405 616263 060708 0 3 090A0B 0 3 0 4 4 2 3 5")
    );

    // Note how it does not matter how big individual items are, as long as total size does not exceed 32 bytes for this example
    let s = "abcdefghijklmnopqrstuvwxyz";
    let obj2 = DynamicThings {
        a: Default::default(),
        b: s,
        c: Default::default(),
    };
    on_stack.set(|wr| obj2.ser_shrink_wrap(wr)).unwrap();
    assert_eq!(on_stack.get().unwrap().b, s);

    // The only caveat here is that buffer usage is higher during serialization (back of the buffer used to temporarily store u16 values),
    // so example above uses full 32 bytes during serialization.
    // But if object is serialized elsewhere with a bigger buffer and only then put into StackVec, then higher utilization is possible:
    let mut buf = [0u8; 36];
    let mut wr = BufWriter::new(&mut buf);
    let s2 = "abcdefghijklmnopqrstuvwxyz0123"; // 4 more bytes fitted into the same StackVec of 32 bytes
    let obj3 = DynamicThings {
        a: Default::default(),
        b: s2,
        c: Default::default(),
    };
    obj3.ser_shrink_wrap(&mut wr).unwrap();
    let bytes = wr.finish_and_take().unwrap();
    on_stack.set_bytes(bytes).unwrap();
    assert_eq!(on_stack.get().unwrap().b, s2);
}
