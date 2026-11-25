| Type | Value | Bytes |
|---|---|---|
| `Option<u8>` | `Some(5)` | `80 05` |
| `Option<u8>` | `None` | `00` |
| `Result<u8, u8>` | `Ok(1)` | `80 01` |
| `Result<u8, u8>` | `Err(2)` | `00 02` |
