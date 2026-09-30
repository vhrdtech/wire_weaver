//! API identity string: which API a device implements, readable without opening the device.
//!
//! Format: `ww:<crate>@<version> h=<hash>[ l=<label>]`, for example:
//! `ww:blinky_api@0.1.0 h=042c28cc0c9da99b l=Kitchen lamp`.
//!
//! * `<crate>@<version>` - API crate name and SemVer version (`FullVersion` of the API root).
//! * `h=` - first 8 bytes of the API hash without docs (`ApiHashPair::no_docs`), lower-case hex.
//! * `l=` - optional user label, always the last field, extends to the end of the string and may contain anything.
//!
//! Fields are separated by a single space, unknown `key=value` fields are ignored by [parse], so that more of them
//! can be added later (before `l=`).
//!
//! On USB this string is used as the WireWeaver interface string (iInterface), which operating systems read during
//! enumeration and expose alongside manufacturer, product and serial strings. Server codegen emits it as the
//! `API_ID` constant, see [api_id_string!](crate::api_id_string) to make one by hand and [with_label] to append a label.

use ww_version::{FullVersion, Version};

/// Maximum USB string descriptor length in UTF-16 code units (bLength is u8: 2 + 2 * 126 = 254).
pub const USB_STRING_MAX_CHARS: usize = 126;
/// Number of API hash bytes included into the string.
pub const HASH_LEN: usize = 8;

const PREFIX: &[u8] = b"ww:";
const HASH_KEY: &[u8] = b" h=";
const LABEL_KEY: &[u8] = b" l=";

/// Parsed API identity string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiId<'i> {
    pub crate_id: &'i str,
    pub version: Version<'i>,
    /// First [HASH_LEN] bytes of the API hash without docs.
    pub hash: Option<[u8; HASH_LEN]>,
    pub label: Option<&'i str>,
}

/// Creates API identity string in const context, evaluates to `&'static str`.
///
/// ```
/// use wire_weaver::ww_version::{FullVersion, Version};
/// const API: FullVersion = FullVersion::new("blinky_api", Version::new(0, 1, 0));
/// const HASH: [u8; 8] = [0x04, 0x2c, 0x28, 0xcc, 0x0c, 0x9d, 0xa9, 0x9b];
/// const ID: &str = wire_weaver::api_id_string!(API, HASH);
/// assert_eq!(ID, "ww:blinky_api@0.1.0 h=042c28cc0c9da99b");
/// ```
///
/// Fails to compile if the result is longer than [USB_STRING_MAX_CHARS].
#[macro_export]
macro_rules! api_id_string {
    ($full_version:expr, $hash:expr) => {{
        const LEN: usize = $crate::api_id::encoded_len(&$full_version, &$hash);
        const BYTES: [u8; LEN] = $crate::api_id::encode::<LEN>(&$full_version, &$hash);
        const ID: &str = match core::str::from_utf8(&BYTES) {
            Ok(s) => s,
            Err(_) => panic!("API id string is not UTF-8"),
        };
        ID
    }};
}

/// Length of the string produced by [encode].
pub const fn encoded_len(api: &FullVersion<'_>, hash: &[u8]) -> usize {
    let mut len = PREFIX.len() + api.crate_id.len() + 1 + version_len(&api.version);
    if !hash.is_empty() {
        len += HASH_KEY.len() + 2 * min(hash.len(), HASH_LEN);
    }
    assert!(
        len <= USB_STRING_MAX_CHARS,
        "API id string does not fit into USB string descriptor"
    );
    len
}

/// Writes API identity string in const context, `N` must be equal to [encoded_len], prefer to use
/// [api_id_string!](crate::api_id_string) instead.
pub const fn encode<const N: usize>(api: &FullVersion<'_>, hash: &[u8]) -> [u8; N] {
    let mut buf = [0u8; N];
    let mut pos = copy(&mut buf, 0, PREFIX);
    pos = copy(&mut buf, pos, api.crate_id.as_bytes());
    pos = copy(&mut buf, pos, b"@");
    let v = &api.version;
    pos = write_u32(&mut buf, pos, v.major.0);
    pos = copy(&mut buf, pos, b".");
    pos = write_u32(&mut buf, pos, v.minor.0);
    pos = copy(&mut buf, pos, b".");
    pos = write_u32(&mut buf, pos, v.patch.0);
    if let Some(pre) = v.pre {
        pos = copy(&mut buf, pos, b"-");
        pos = copy(&mut buf, pos, pre.as_bytes());
    }
    if let Some(build) = v.build {
        pos = copy(&mut buf, pos, b"+");
        pos = copy(&mut buf, pos, build.as_bytes());
    }
    if !hash.is_empty() {
        pos = copy(&mut buf, pos, HASH_KEY);
        let mut i = 0;
        while i < min(hash.len(), HASH_LEN) {
            buf[pos] = hex_digit(hash[i] >> 4);
            buf[pos + 1] = hex_digit(hash[i] & 0xf);
            pos += 2;
            i += 1;
        }
    }
    assert!(pos == N, "wrong API id string length");
    buf
}

/// Copies `api_id` and appends `label` to it (if not empty) into `buf`.
///
/// Control characters in the label are replaced with spaces. Label is truncated if the result does not fit into
/// `buf` or is longer than [USB_STRING_MAX_CHARS] UTF-16 code units. `api_id` is returned unchanged if `buf` is too
/// short for it.
pub fn with_label<'b>(api_id: &str, label: &str, buf: &'b mut [u8]) -> &'b str {
    let label = label.trim();
    let base_len = api_id.len() + LABEL_KEY.len();
    if label.is_empty() || base_len > buf.len() {
        let len = if api_id.len() <= buf.len() {
            api_id.len()
        } else {
            0
        };
        buf[..len].copy_from_slice(&api_id.as_bytes()[..len]);
        return core::str::from_utf8(&buf[..len]).unwrap_or_default();
    }
    buf[..api_id.len()].copy_from_slice(api_id.as_bytes());
    buf[api_id.len()..base_len].copy_from_slice(LABEL_KEY);
    let mut pos = base_len;
    let mut utf16_len = api_id.encode_utf16().count() + LABEL_KEY.len();
    for c in label.chars() {
        let c = if c.is_control() { ' ' } else { c };
        if pos + c.len_utf8() > buf.len() || utf16_len + c.len_utf16() > USB_STRING_MAX_CHARS {
            break;
        }
        c.encode_utf8(&mut buf[pos..]);
        pos += c.len_utf8();
        utf16_len += c.len_utf16();
    }
    // only whole chars were written
    core::str::from_utf8(&buf[..pos]).unwrap_or_default()
}

/// Parses API identity string, returns None if it is not one.
pub fn parse(s: &str) -> Option<ApiId<'_>> {
    let rest = s.strip_prefix("ww:")?;
    let (head, mut fields) = rest.split_once(' ').unwrap_or((rest, ""));
    let (crate_id, version) = head.split_once('@')?;
    if crate_id.is_empty() {
        return None;
    }
    let version = parse_version(version)?;
    let mut hash = None;
    let mut label = None;
    while !fields.is_empty() {
        if let Some(l) = fields.strip_prefix("l=") {
            label = Some(l);
            break;
        }
        let (field, rest) = fields.split_once(' ').unwrap_or((fields, ""));
        if let Some(h) = field.strip_prefix("h=") {
            hash = parse_hash(h);
        }
        fields = rest;
    }
    Some(ApiId {
        crate_id,
        version,
        hash,
        label,
    })
}

fn parse_version(s: &str) -> Option<Version<'_>> {
    let (s, build) = match s.split_once('+') {
        Some((s, build)) => (s, Some(build)),
        None => (s, None),
    };
    let (s, pre) = match s.split_once('-') {
        Some((s, pre)) => (s, Some(pre)),
        None => (s, None),
    };
    let mut parts = s.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(Version::full(major, minor, patch, pre, build))
}

fn parse_hash(s: &str) -> Option<[u8; HASH_LEN]> {
    let s = s.as_bytes();
    if s.len() != 2 * HASH_LEN {
        return None;
    }
    let mut hash = [0u8; HASH_LEN];
    for (i, byte) in hash.iter_mut().enumerate() {
        *byte = (hex_value(s[2 * i])? << 4) | hex_value(s[2 * i + 1])?;
    }
    Some(hash)
}

const fn version_len(v: &Version<'_>) -> usize {
    let mut len = u32_len(v.major.0) + 1 + u32_len(v.minor.0) + 1 + u32_len(v.patch.0);
    if let Some(pre) = v.pre {
        len += 1 + pre.len();
    }
    if let Some(build) = v.build {
        len += 1 + build.len();
    }
    len
}

const fn u32_len(mut x: u32) -> usize {
    let mut len = 1;
    while x >= 10 {
        x /= 10;
        len += 1;
    }
    len
}

const fn write_u32(buf: &mut [u8], pos: usize, mut x: u32) -> usize {
    let len = u32_len(x);
    let mut i = len;
    while i > 0 {
        i -= 1;
        buf[pos + i] = b'0' + (x % 10) as u8;
        x /= 10;
    }
    pos + len
}

const fn copy(buf: &mut [u8], pos: usize, bytes: &[u8]) -> usize {
    let mut i = 0;
    while i < bytes.len() {
        buf[pos + i] = bytes[i];
        i += 1;
    }
    pos + bytes.len()
}

const fn hex_digit(nibble: u8) -> u8 {
    if nibble < 10 {
        b'0' + nibble
    } else {
        b'a' + nibble - 10
    }
}

fn hex_value(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

const fn min(a: usize, b: usize) -> usize {
    if a < b { a } else { b }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: [u8; 8] = [0x04, 0x2c, 0x28, 0xcc, 0x0c, 0x9d, 0xa9, 0x9b];

    #[test]
    fn encode_simple() {
        const API: FullVersion = FullVersion::new("blinky_api", Version::new(0, 1, 0));
        const ID: &str = crate::api_id_string!(API, HASH);
        assert_eq!(ID, "ww:blinky_api@0.1.0 h=042c28cc0c9da99b");
        let parsed = parse(ID).unwrap();
        assert_eq!(
            parsed,
            ApiId {
                crate_id: "blinky_api",
                version: Version::new(0, 1, 0),
                hash: Some(HASH),
                label: None
            }
        );
    }

    #[test]
    fn encode_pre_build_no_hash() {
        const API: FullVersion = FullVersion::new(
            "my-api",
            Version::full(12, 345, 6789, Some("alpha.1"), Some("zstd.1.5")),
        );
        const ID: &str = crate::api_id_string!(API, []);
        assert_eq!(ID, "ww:my-api@12.345.6789-alpha.1+zstd.1.5");
        let parsed = parse(ID).unwrap();
        assert_eq!(parsed.crate_id, "my-api");
        assert_eq!(parsed.version, API.version);
        assert_eq!(parsed.hash, None);
    }

    #[test]
    fn label() {
        const API: FullVersion = FullVersion::new("blinky_api", Version::new(1, 0, 0));
        const ID: &str = crate::api_id_string!(API, HASH);
        let mut buf = [0u8; 256];
        let s = with_label(ID, " Kitchen\tlamp h=00 l=x ", &mut buf);
        assert_eq!(
            s,
            "ww:blinky_api@1.0.0 h=042c28cc0c9da99b l=Kitchen lamp h=00 l=x"
        );
        let parsed = parse(s).unwrap();
        assert_eq!(parsed.hash, Some(HASH));
        assert_eq!(parsed.label, Some("Kitchen lamp h=00 l=x"));

        let s = with_label(ID, "", &mut buf);
        assert_eq!(s, ID);
    }

    #[test]
    fn label_truncated() {
        const API: FullVersion = FullVersion::new("blinky_api", Version::new(1, 0, 0));
        const ID: &str = crate::api_id_string!(API, HASH);
        let mut buf = [0u8; 512];
        let label = "ы".repeat(200);
        let s = with_label(ID, &label, &mut buf);
        assert_eq!(s.encode_utf16().count(), USB_STRING_MAX_CHARS);

        let mut buf = [0u8; 50];
        let s = with_label(ID, &label, &mut buf);
        // 9 bytes left after "<ID> l=", fits 4 two-byte chars
        assert_eq!(s.len(), 49);
        assert!(s.starts_with(ID));

        let mut buf = [0u8; 10];
        assert_eq!(with_label(ID, &label, &mut buf), "");
    }

    #[test]
    fn unknown_fields_ignored() {
        let parsed = parse("ww:a@1.2.3 x=future h=042c28cc0c9da99b y l=lbl").unwrap();
        assert_eq!(parsed.hash, Some(HASH));
        assert_eq!(parsed.label, Some("lbl"));
    }

    #[test]
    fn not_an_id() {
        assert!(parse("WireWeaver Generic").is_none());
        assert!(parse("ww:").is_none());
        assert!(parse("ww:a").is_none());
        assert!(parse("ww:@1.0.0").is_none());
        assert!(parse("ww:a@1.0").is_none());
        assert!(parse("ww:a@1.0.0.0").is_none());
        assert_eq!(parse("ww:a@1.0.0 h=zz").unwrap().hash, None);
    }
}
