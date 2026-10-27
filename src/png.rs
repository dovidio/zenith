//! A minimal PNG writer (no dependencies).
//!
//! Zenith uses this for one thing: the `--smoke` self-portrait, so that the app can
//! prove to you (and to whoever reviews it) what it actually rendered. It writes
//! uncompressed zlib blocks, which is wasteful and completely sufficient.

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, entry) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *entry = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], payload: &[u8]) {
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(payload);
    let mut crc_input = Vec::with_capacity(4 + payload.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(payload);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

/// `rgb` is row-major, 3 bytes per pixel, length = w * h * 3.
pub fn encode_rgb(width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
    assert_eq!(rgb.len(), (width * height * 3) as usize);

    // Raw scanlines, each prefixed with filter type 0.
    let stride = (width * 3) as usize;
    let mut raw = Vec::with_capacity((stride + 1) * height as usize);
    for y in 0..height as usize {
        raw.push(0u8);
        raw.extend_from_slice(&rgb[y * stride..(y + 1) * stride]);
    }

    // zlib, stored blocks (max 65535 bytes each).
    let mut z = Vec::with_capacity(raw.len() + raw.len() / 65535 * 5 + 16);
    z.extend_from_slice(&[0x78, 0x01]);
    let mut offset = 0usize;
    while offset < raw.len() {
        let len = (raw.len() - offset).min(65535);
        let last = if offset + len >= raw.len() { 1u8 } else { 0u8 };
        z.push(last);
        z.extend_from_slice(&(len as u16).to_le_bytes());
        z.extend_from_slice(&(!(len as u16)).to_le_bytes());
        z.extend_from_slice(&raw[offset..offset + len]);
        offset += len;
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut out = Vec::with_capacity(z.len() + 64);
    out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit, truecolor RGB
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_a_header() {
        let png = encode_rgb(2, 2, &[0u8; 12]);
        assert_eq!(&png[1..4], b"PNG");
        assert_eq!(&png[12..16], b"IHDR");
    }
}
