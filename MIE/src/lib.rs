use std::io::{Read, Seek, SeekFrom};

pub struct Span {
    pub start: u64,
    pub length: u64,
}

/// True when `buf` ends with one of the four `zmie` trailer signatures.
pub fn ends_with_signature(buf: &[u8]) -> bool {
    signature4(buf) || signature8(buf)
}

/// Group length stored just before the last two bytes of a `zmie` signature.
pub fn group_length(buf: &[u8]) -> Option<u64> {
    let n = buf.len();
    if signature4(buf) {
        Some(u32_at(buf, n - 6, buf[n - 2] == 0x10) as u64)
    } else if signature8(buf) {
        Some(u64_at(buf, n - 10, buf[n - 2] == 0x10))
    } else {
        None
    }
}

/// Walk MIE group terminators backward from EOF.
///
/// On success the reader is left at `start`. The walk does not require `zmie`.
/// A `Tell` of 0, a short read, or a failed seek ends the walk.
pub fn locate<R: Read + Seek>(reader: &mut R, offset_from_eof: u64) -> Option<Span> {
    let back = 10u64.checked_add(offset_from_eof)?;
    let back = i64::try_from(back).ok()?;
    reader.seek(SeekFrom::End(-back)).ok()?;

    let mut pos = None;
    let mut end = None;
    let mut window = [0u8; 10];
    loop {
        if read_n(reader, &mut window) != 10 {
            break;
        }
        let Some((wide, len)) = terminator(&window) else {
            break;
        };
        let cur = match reader.stream_position() {
            Ok(0) | Err(_) => break,
            Ok(cur) => cur,
        };
        if len < 12 || len > cur {
            break;
        }
        if wide {
            if len < 14 {
                break;
            }
            if reader.seek(SeekFrom::Start(cur - 14)).is_err() || read_n(reader, &mut window[..4]) != 4
            {
                break;
            }
            if &window[..4] != b"~\0\0\x0a" {
                break;
            }
        }
        let start = cur - len;
        if end.is_none() {
            end = Some(cur);
        }
        pos = Some(start);
        if start < 10 || reader.seek(SeekFrom::Start(start - 10)).is_err() {
            break;
        }
    }

    let start = pos?;
    let end = end?;
    reader.seek(SeekFrom::Start(start)).ok()?;
    Some(Span {
        start,
        length: end - start,
    })
}

/// Host bytes with a signed trailer removed. Unchanged when there is no trailer.
pub fn strip(bytes: &[u8]) -> Vec<u8> {
    if !ends_with_signature(bytes) {
        return bytes.to_vec();
    }
    match locate(&mut std::io::Cursor::new(bytes), 0) {
        Some(span) => bytes[..span.start as usize].to_vec(),
        None => bytes.to_vec(),
    }
}

/// Append a MIE document that already ends with the `zmie` signature.
pub fn append(host: &[u8], mie: &[u8]) -> Option<Vec<u8>> {
    if !ends_with_signature(mie) {
        return None;
    }
    let mut out = Vec::with_capacity(host.len() + mie.len());
    out.extend_from_slice(host);
    out.extend_from_slice(mie);
    Some(out)
}

fn signature4(buf: &[u8]) -> bool {
    let n = buf.len();
    if n < 18 {
        return false;
    }
    let s = &buf[n - 18..];
    s[0] == b'~'
        && s[1] == 0
        && s[2] == 0x04
        && s[3] == 0
        && &s[4..8] == b"zmie"
        && s[8] == b'~'
        && s[9] == 0
        && s[10] == 0
        && s[11] == 0x06
        && (s[16] == 0x10 || s[16] == 0x18)
        && s[17] == 0x04
}

fn signature8(buf: &[u8]) -> bool {
    let n = buf.len();
    if n < 22 {
        return false;
    }
    let s = &buf[n - 22..];
    s[0] == b'~'
        && s[1] == 0
        && s[2] == 0x04
        && s[3] == 0
        && &s[4..8] == b"zmie"
        && s[8] == b'~'
        && s[9] == 0
        && s[10] == 0
        && s[11] == 0x0a
        && (s[20] == 0x10 || s[20] == 0x18)
        && s[21] == 0x08
}

/// `(wide, length)` for a 10-byte terminator window. The 4-byte form wins.
fn terminator(window: &[u8; 10]) -> Option<(bool, u64)> {
    let order = window[8];
    if window[0] == b'~'
        && window[1] == 0
        && window[2] == 0
        && window[3] == 0x06
        && (order == 0x10 || order == 0x18)
        && window[9] == 0x04
    {
        return Some((false, u32_at(window, 4, order == 0x10) as u64));
    }
    if (order == 0x10 || order == 0x18) && window[9] == 0x08 {
        return Some((true, u64_at(window, 0, order == 0x10)));
    }
    None
}

fn u32_at(buf: &[u8], off: usize, be: bool) -> u32 {
    let b = buf[off..off + 4].try_into().unwrap();
    if be {
        u32::from_be_bytes(b)
    } else {
        u32::from_le_bytes(b)
    }
}

fn u64_at(buf: &[u8], off: usize, be: bool) -> u64 {
    let b = buf[off..off + 8].try_into().unwrap();
    if be {
        u64::from_be_bytes(b)
    } else {
        u64::from_le_bytes(b)
    }
}

fn read_n<R: Read>(reader: &mut R, buf: &mut [u8]) -> usize {
    let mut got = 0;
    while got < buf.len() {
        match reader.read(&mut buf[got..]) {
            Ok(0) | Err(_) => break,
            Ok(n) => got += n,
        }
    }
    got
}

/// Big-endian file-level `0MIE` group. `payload` is a UTF-8 element named `Pin`.
/// The last element is the empty `zmie` trailer signature.
pub fn document(payload: &[u8]) -> Vec<u8> {
    let mut body = element(0x28, b"Pin", payload);
    body.extend_from_slice(&element(0x00, b"zmie", b""));
    finish_group(&body)
}

/// Drop a signed trailer, then append `payload` as a new MIE document.
pub fn embed(host: &[u8], payload: &[u8]) -> Vec<u8> {
    let host = strip(host);
    let mie = document(payload);
    let mut out = Vec::with_capacity(host.len() + mie.len());
    out.extend_from_slice(&host);
    out.extend_from_slice(&mie);
    out
}

/// `Pin` bytes from a trailer written by [`document`].
///
/// Accepts one big-endian file-level `0MIE` group: a UTF-8 `Pin` element,
/// an empty `zmie`, and the group terminator. The slice is borrowed from `bytes`.
pub fn pin(bytes: &[u8]) -> Option<&[u8]> {
    if !ends_with_signature(bytes) {
        return None;
    }
    let span = locate(&mut std::io::Cursor::new(bytes), 0)?;
    if group_length(bytes)? != span.length {
        return None;
    }
    let start = usize::try_from(span.start).ok()?;
    let len = usize::try_from(span.length).ok()?;
    if start.checked_add(len)? != bytes.len() {
        return None;
    }
    read_pin(&bytes[start..])
}

struct Item<'a> {
    format: u8,
    name: &'a [u8],
    data: &'a [u8],
}

fn read_pin(doc: &[u8]) -> Option<&[u8]> {
    if doc.len() < 8 || doc[0] != b'~' || doc[1] != 0x10 || doc[2] != 4 || &doc[4..8] != b"0MIE" {
        return None;
    }
    // `document` stores the group length as a big-endian u32 (`0xfe`) or u64 (`0xfd`).
    if doc[3] != 0xfe && doc[3] != 0xfd {
        return None;
    }
    let mut at = 0;
    let group = take_element(doc, &mut at)?;
    if at != doc.len() {
        return None;
    }
    let mut inner = 0;
    let pin = take_element(group.data, &mut inner)?;
    let zmie = take_element(group.data, &mut inner)?;
    let term = take_element(group.data, &mut inner)?;
    if inner != group.data.len() {
        return None;
    }
    if pin.format != 0x28 || pin.name != b"Pin" {
        return None;
    }
    if zmie.format != 0 || zmie.name != b"zmie" || !zmie.data.is_empty() {
        return None;
    }
    if term.format != 0 || !term.name.is_empty() {
        return None;
    }
    Some(pin.data)
}

/// One element at `at`. Extended lengths are big-endian and follow the tag name.
fn take_element<'a>(buf: &'a [u8], at: &mut usize) -> Option<Item<'a>> {
    let start = *at;
    let rest = buf.get(start..)?;
    if rest.len() < 4 || rest[0] != b'~' {
        return None;
    }
    let format = rest[1];
    let tag_len = rest[2] as usize;
    let name_end = 4usize.checked_add(tag_len)?;
    let name = rest.get(4..name_end)?;
    let (data_len, data_at) = data_len_at(rest, name_end, rest[3])?;
    let data_end = data_at.checked_add(data_len)?;
    let data = rest.get(data_at..data_end)?;
    *at = start + data_end;
    Some(Item { format, name, data })
}

/// `(byte length, index of the data)`. `0xfd`, `0xfe`, and `0xff` are extended lengths.
fn data_len_at(buf: &[u8], ext_at: usize, code: u8) -> Option<(usize, usize)> {
    match code {
        0xfd => {
            let raw = buf.get(ext_at..ext_at.checked_add(8)?)?;
            let n = u64::from_be_bytes(raw.try_into().ok()?);
            Some((usize::try_from(n).ok()?, ext_at + 8))
        }
        0xfe => {
            let raw = buf.get(ext_at..ext_at.checked_add(4)?)?;
            let n = u32::from_be_bytes(raw.try_into().ok()?);
            Some((n as usize, ext_at + 4))
        }
        0xff => {
            let raw = buf.get(ext_at..ext_at.checked_add(2)?)?;
            let n = u16::from_be_bytes(raw.try_into().ok()?);
            Some((n as usize, ext_at + 2))
        }
        _ => Some((code as usize, ext_at)),
    }
}

/// One element. Extended lengths use the enclosing group's big-endian order.
fn element(format: u8, tag: &[u8], data: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(16 + tag.len() + data.len());
    v.push(b'~');
    v.push(format);
    v.push(tag.len() as u8);
    let n = data.len();
    if n <= 252 {
        v.push(n as u8);
        v.extend_from_slice(tag);
    } else if n <= 0xffff {
        v.push(0xff);
        v.extend_from_slice(tag);
        v.extend_from_slice(&(n as u16).to_be_bytes());
    } else if n <= 0xffff_ffff {
        v.push(0xfe);
        v.extend_from_slice(tag);
        v.extend_from_slice(&(n as u32).to_be_bytes());
    } else {
        v.push(0xfd);
        v.extend_from_slice(tag);
        v.extend_from_slice(&(n as u64).to_be_bytes());
    }
    v.extend_from_slice(data);
    v
}

/// Opening `0MIE` element plus a file-level terminator. GroupLength covers both.
fn finish_group(body: &[u8]) -> Vec<u8> {
    let body_len = body.len() as u64;
    let narrow = body_len + 22;
    if narrow <= u32::MAX as u64 {
        let data_len = (body_len + 10) as u32;
        let group_len = narrow as u32;
        let mut out = Vec::with_capacity(group_len as usize);
        push_group_header(&mut out, 0xfe, &data_len.to_be_bytes());
        out.extend_from_slice(body);
        push_term(&mut out, &group_len.to_be_bytes(), 0x04);
        out
    } else {
        let data_len = body_len + 14;
        let group_len = data_len + 16;
        let mut out = Vec::with_capacity(group_len as usize);
        push_group_header(&mut out, 0xfd, &data_len.to_be_bytes());
        out.extend_from_slice(body);
        push_term(&mut out, &group_len.to_be_bytes(), 0x08);
        out
    }
}

fn push_group_header(out: &mut Vec<u8>, len_code: u8, len: &[u8]) {
    out.push(b'~');
    out.push(0x10);
    out.push(0x04);
    out.push(len_code);
    out.extend_from_slice(b"0MIE");
    out.extend_from_slice(len);
}

fn push_term(out: &mut Vec<u8>, group_len: &[u8], size_code: u8) {
    out.push(b'~');
    out.push(0);
    out.push(0);
    out.push(if size_code == 0x04 { 0x06 } else { 0x0a });
    out.extend_from_slice(group_len);
    out.push(0x10);
    out.push(size_code);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read, Seek, SeekFrom};

    const ZMIE: &[u8] = b"~\0\x04\0zmie";

    fn term4(len: u32, order: u8) -> Vec<u8> {
        let mut v = vec![b'~', 0, 0, 0x06];
        let raw = if order == 0x10 {
            len.to_be_bytes()
        } else {
            len.to_le_bytes()
        };
        v.extend_from_slice(&raw);
        v.push(order);
        v.push(0x04);
        v
    }

    fn group4(order: u8, body: &[u8]) -> Vec<u8> {
        let mut v = body.to_vec();
        v.extend(term4((body.len() + 10) as u32, order));
        v
    }

    fn term8(len: u64, order: u8) -> Vec<u8> {
        let mut v = vec![b'~', 0, 0, 0x0a];
        let raw = if order == 0x10 {
            len.to_be_bytes()
        } else {
            len.to_le_bytes()
        };
        v.extend_from_slice(&raw);
        v.push(order);
        v.push(0x08);
        v
    }

    fn group8(order: u8, body: &[u8]) -> Vec<u8> {
        let mut v = body.to_vec();
        v.extend(term8((body.len() + 14) as u64, order));
        v
    }

    /// Reads at most one byte per `read` call.
    struct Short<T>(T);

    impl<T: Read> Read for Short<T> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if buf.is_empty() {
                return Ok(0);
            }
            self.0.read(&mut buf[..1])
        }
    }

    impl<T: Seek> Seek for Short<T> {
        fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
            self.0.seek(pos)
        }
    }

    #[test]
    fn four_signatures_match_and_a_bare_terminator_does_not() {
        let be4 = [&[b'Q'; 40][..], b"~\0\x04\0zmie~\0\0\x06", &0x01020304u32.to_be_bytes(), b"\x10\x04"].concat();
        let le4 = [&[b'Q'; 40][..], b"~\0\x04\0zmie~\0\0\x06", &0x01020304u32.to_le_bytes(), b"\x18\x04"].concat();
        let wide = (0x01020304u64 << 32) | 0x05060708;
        let be8 = [&[b'Q'; 30][..], b"~\0\x04\0zmie~\0\0\x0a", &wide.to_be_bytes(), b"\x10\x08"].concat();
        let le8 = [&[b'Q'; 30][..], b"~\0\x04\0zmie~\0\0\x0a", &wide.to_le_bytes(), b"\x18\x08"].concat();
        let bare = [&[b'Q'; 20][..], b"~\0\0\x06", &30u32.to_be_bytes(), b"\x10\x04"].concat();

        assert!(ends_with_signature(&be4));
        assert!(ends_with_signature(&le4));
        assert!(ends_with_signature(&be8));
        assert!(ends_with_signature(&le8));
        assert!(!ends_with_signature(&bare));

        assert_eq!(group_length(&be4), Some(0x01020304));
        assert_eq!(group_length(&le4), Some(0x01020304));
        assert_eq!(group_length(&be8), Some(wide));
        assert_eq!(group_length(&le8), Some(wide));
        assert_eq!(group_length(&bare), None);
    }

    #[test]
    fn locate_one_group_two_groups_and_eight_byte_group() {
        let host = b"HOST";
        let g4 = group4(0x10, ZMIE);
        let buf = [host, g4.as_slice()].concat();
        let mut cur = Cursor::new(buf.clone());
        let one = locate(&mut cur, 0).unwrap();
        assert_eq!(one.start, host.len() as u64);
        assert_eq!(one.length, g4.len() as u64);
        assert_eq!(cur.position(), one.start);

        let mut short = Short(Cursor::new(buf));
        let via_short = locate(&mut short, 0).unwrap();
        assert_eq!(via_short.start, one.start);
        assert_eq!(via_short.length, one.length);

        let g2 = group4(0x18, &[ZMIE, b"more"].concat());
        let two = locate(&mut Cursor::new([host, g4.as_slice(), g2.as_slice()].concat()), 0).unwrap();
        assert_eq!(two.start, host.len() as u64);
        assert_eq!(two.length, (g4.len() + g2.len()) as u64);

        let g8 = group8(0x18, ZMIE);
        let wide = locate(&mut Cursor::new([host, g8.as_slice()].concat()), 0).unwrap();
        assert_eq!(wide.start, host.len() as u64);
        assert_eq!(wide.length, g8.len() as u64);
    }

    #[test]
    fn locate_rejects_a_length_past_the_start_a_short_read_and_a_bad_header() {
        let past = [&[b'X'; 10][..], b"~\0\0\x06", &1000u32.to_be_bytes(), b"\x10\x04"].concat();
        assert!(locate(&mut Cursor::new(past), 0).is_none());
        assert!(locate(&mut Cursor::new(b"short".as_slice()), 0).is_none());

        let mut bad = vec![b'A'; 16];
        bad.extend(b"XXXX");
        bad.extend_from_slice(&20u64.to_be_bytes());
        bad.extend_from_slice(b"\x10\x08");
        assert!(locate(&mut Cursor::new(bad), 0).is_none());
    }

    #[test]
    fn strip_and_append() {
        let host = b"HOST";
        let g4 = group4(0x10, ZMIE);
        let signed = [host, g4.as_slice()].concat();
        assert_eq!(strip(&signed), host);
        assert_eq!(strip(host), host);

        let nosig = [host, group4(0x10, b"ABCDEFGH").as_slice()].concat();
        assert_eq!(strip(&nosig), nosig);

        assert_eq!(append(host, &g4).unwrap(), signed);
        assert!(append(host, b"nope").is_none());
    }

    #[test]
    fn locate_honors_a_suffix_after_the_trailer() {
        let host = b"HOST";
        let g4 = group4(0x10, ZMIE);
        let buf = [host, g4.as_slice(), b"SUFFIX"].concat();
        let found = locate(&mut Cursor::new(buf), 6).unwrap();
        assert_eq!(found.start, host.len() as u64);
        assert_eq!(found.length, g4.len() as u64);
    }

    #[test]
    fn document_embeds_utf8_and_replaces_a_trailer() {
        let payload = "Shop Now →".as_bytes();
        let doc = document(payload);
        assert!(ends_with_signature(&doc));
        assert_eq!(group_length(&doc), Some(doc.len() as u64));
        assert!(doc.starts_with(b"~\x10\x04\xfe"));
        assert_eq!(&doc[4..8], b"0MIE");
        assert_eq!(
            u32::from_be_bytes(doc[8..12].try_into().unwrap()) as usize,
            doc.len() - 12
        );
        let found = locate(&mut Cursor::new(&doc), 0).unwrap();
        assert_eq!(found.start, 0);
        assert_eq!(found.length, doc.len() as u64);
        assert!(doc.windows(payload.len()).any(|w| w == payload));

        let wide = vec![0x61u8; 300];
        let wide_doc = document(&wide);
        assert!(ends_with_signature(&wide_doc));
        assert_eq!(group_length(&wide_doc), Some(wide_doc.len() as u64));
        assert!(wide_doc.windows(300).any(|w| w == wide.as_slice()));

        let host = b"\xff\xd8JPEG";
        let once = embed(host, payload);
        assert_eq!(strip(&once), host);
        let twice = embed(&once, &wide);
        assert_eq!(strip(&twice), host);
        assert!(twice.windows(300).any(|w| w == wide.as_slice()));
        assert!(!twice.windows(payload.len()).any(|w| w == payload));
    }

    #[test]
    fn sha256_hex_fits_in_the_one_byte_string_length() {
        // A MIE text value uses a one-byte length for 0..=252 bytes.
        // A SHA-256 image hash in hex ASCII is 64 bytes, same shape as MIE's MD5 hex form.
        let digest = [b'a'; 64];
        assert_eq!(digest.len(), 64);
        assert!(digest.len() <= 252);
    }

    #[test]
    fn pin_round_trips_the_document_payload() {
        let arrow = "Shop Now →".as_bytes();
        let doc = document(arrow);
        assert_eq!(pin(&doc), Some(arrow));
        let host = b"\xff\xd8JPEG";
        assert_eq!(pin(&embed(host, arrow)), Some(arrow));

        let empty = document(b"");
        assert_eq!(empty[15], 0);
        assert_eq!(pin(&empty), Some(b"".as_slice()));

        let one_byte = vec![0x61; 252];
        let one = document(&one_byte);
        assert_eq!(one[15], 252);
        assert_eq!(pin(&one), Some(one_byte.as_slice()));

        let u16_payload = vec![0x62; 300];
        let wide = document(&u16_payload);
        assert_eq!(wide[15], 0xff);
        assert_eq!(pin(&wide), Some(u16_payload.as_slice()));

        let u32_payload = vec![0x63; 65536];
        let huge = document(&u32_payload);
        assert_eq!(huge[15], 0xfe);
        assert_eq!(pin(&huge), Some(u32_payload.as_slice()));

        let once = embed(host, arrow);
        let twice = embed(&once, &u16_payload);
        assert_eq!(pin(&twice), Some(u16_payload.as_slice()));
        assert_eq!(pin(&once), Some(arrow));
    }

    #[test]
    fn pin_rejects_other_shapes() {
        assert_eq!(pin(b"\xff\xd8JPEG"), None);

        let bare = group4(0x10, ZMIE);
        assert!(ends_with_signature(&bare));
        assert_eq!(pin(&bare), None);

        let mut long = document(b"ab");
        long[15] = long[15].saturating_add(1);
        assert_eq!(pin(&long), None);

        let mut le = document(b"ab");
        le[1] = 0x18;
        let data_len = u32::from_be_bytes(le[8..12].try_into().unwrap());
        le[8..12].copy_from_slice(&data_len.to_le_bytes());
        let n = le.len();
        let group_len = u32::from_be_bytes(le[n - 6..n - 2].try_into().unwrap());
        le[n - 6..n - 2].copy_from_slice(&group_len.to_le_bytes());
        le[n - 2] = 0x18;
        assert!(ends_with_signature(&le));
        let found = locate(&mut Cursor::new(le.clone()), 0).unwrap();
        assert_eq!(group_length(&le), Some(found.length));
        assert_eq!(pin(&le), None);

        let two = [document(b"one"), document(b"two")].concat();
        assert!(ends_with_signature(&two));
        assert_eq!(pin(&two), None);
    }

    #[test]
    fn u64_length_code_does_not_allocate_the_declared_buffer() {
        let declared = u64::MAX.to_be_bytes();
        assert_eq!(data_len_at(&declared, 0, 0xfd), Some((usize::MAX, 8)));

        let mut overrun = vec![b'~', 0x28, 3, 0xfd];
        overrun.extend_from_slice(b"Pin");
        overrun.extend_from_slice(&declared);
        let mut at = 0;
        assert!(take_element(&overrun, &mut at).is_none());

        let mut body = vec![b'~', 0x28, 3, 0xfd];
        body.extend_from_slice(b"Pin");
        body.extend_from_slice(&1u64.to_be_bytes());
        body.push(b'Z');
        body.extend_from_slice(&element(0x00, b"zmie", b""));
        let doc = finish_group(&body);
        assert_eq!(doc[15], 0xfd);
        assert_eq!(pin(&doc), Some(b"Z".as_slice()));
    }
}
