# Compare wire formats

This example showcases some of the `shrink_wrap` features that result in dense wire representation of the following data
structures:

```rust
struct Request {
    pub seq: u16,
    pub path_kind: PathKind,
    pub kind: RequestKind,
    pub flags: [bool; 5],
    pub dummy_a: Option<u8>,
    pub dummy_b: Option<u8>,
    pub dummy_c: Option<u8>,
    pub strs: Vec<String>,
}

enum PathKind {
    Absolute { path: Vec<u32> },
    GlobalCompact,
    GlobalFull,
}

enum RequestKind {
    Call { args: Vec<u8> },
    Read,
}
```

With the following example value:

```rust
fn main() {
    let req = RequestSerde {
        seq: 1234,
        path_kind: PathKindSerde::Absolute { path: vec![0, 1, 2] },
        kind: RequestKindSerde::Call { args: vec![0xAA, 0xBB, 0xCC] },
        flags: [false, true, false, true, false],
        dummy_a: Some(0xCC),
        dummy_b: None,
        dummy_c: None,
        strs: vec!["ab".to_string(), "c".to_string()],
    };
}
```

In particular the following features are resulting in a considerably smaller size:

* UNib32 encoding using nibbles, for path array and for all lengths:
    * `D2 04` is u16 in LE
    * `00 12` first nibble is PathKind discriminant, then `0, 1, 2` is path array, nibbles are 4-bit aligned, so the two
      stick together
    * `00` first nibbles is RequestKind discriminant, second one is padding before u8 args array
    * From the back: `01 22 33` are the lengths of the arrays and strings - 3, 3, 2, 2, 1, padding
* Boolean values stored as bits - `0b0010_1010 = 0x2A`
* Option's flag grouping - see the source on how the flags for Option's are relocated to be grouped with the bool array.

Results:

```
WireWeaver: len: 16: [D2, 04, 00, 12, 00, AA, BB, CC, 2A, CC, 61, 62, 63, 01, 22, 33]
Postcard: len: 27: [D2, 09, 00, 03, 00, 01, 02, 00, 03, AA, BB, CC, 00, 01, 00, 01, 00, 01, CC, 00, 00, 02, 02, 61, 62, 01, 63]
Bincode: len: 28: [FB, D2, 04, 00, 03, 00, 01, 02, 00, 03, AA, BB, CC, 00, 01, 00, 01, 00, 01, CC, 00, 00, 02, 02, 61, 62, 01, 63]
MessagePack: len: 49: [98, CD, 04, D2, 81, A8, 41, 62, 73, 6F, 6C, 75, 74, 65, 91, 93, 00, 01, 02, 81, A4, 43, 61, 6C, 6C, 91, 93, CC, AA, CC, BB, CC, CC, 95, C2, C3, C2, C3, C2, CC, CC, C0, C0, 92, A2, 61, 62, A1, 63]
JSON: len: 193: {"seq":1234,"path_kind":{"Absolute":{"path":[0,1,2]}},"kind":{"Call":{"args":[170,187,204]}},"flags":[false,true,false,true,false],"dummy_a":204,"dummy_b":null,"dummy_c":null,"strs":["ab","c"]}
```