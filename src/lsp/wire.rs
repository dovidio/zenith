//! JSON-RPC framing: a header block, a blank line, then exactly as many bytes of body
//! as the header announced.
//!
//! Four lines of protocol, with a failure mode that is not small: read one byte wrong
//! and every message after it is nonsense, in a way that looks like a server that has
//! gone quiet. So it is its own file, it takes a reader and a writer, and it is tested
//! without a server.

use std::io::{BufRead, Write};

/// Write one message. The length is of the *bytes*, not of the characters.
pub fn write<W: Write>(out: &mut W, body: &str) -> std::io::Result<()> {
    write!(out, "Content-Length: {}\r\n\r\n", body.len())?;
    out.write_all(body.as_bytes())?;
    out.flush()
}

/// Read one message, or `None` when the stream has ended cleanly.
pub fn read<R: BufRead>(input: &mut R) -> std::io::Result<Option<Vec<u8>>> {
    let mut length: Option<usize> = None;
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let header = line.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            break;
        }
        if let Some(value) = header
            .strip_prefix("Content-Length:")
            .or_else(|| header.strip_prefix("content-length:"))
        {
            length = value.trim().parse().ok();
        }
        // `Content-Type` is the only other header any server sends, and every server
        // zenith speaks to sends JSON.
    }
    let Some(length) = length else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "a message with no Content-Length",
        ));
    };
    let mut body = vec![0u8; length];
    input.read_exact(&mut body)?;
    Ok(Some(body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn a_message_survives_the_round_trip() {
        let mut buffer: Vec<u8> = Vec::new();
        write(&mut buffer, r#"{"id":1}"#).unwrap();
        assert_eq!(buffer, b"Content-Length: 8\r\n\r\n{\"id\":1}");
        let mut input = Cursor::new(buffer);
        assert_eq!(
            read(&mut input).unwrap().as_deref(),
            Some(&b"{\"id\":1}"[..])
        );
        assert_eq!(read(&mut input).unwrap(), None, "a clean end of stream");
    }

    #[test]
    fn back_to_back_messages_do_not_bleed_into_each_other() {
        let mut buffer: Vec<u8> = Vec::new();
        write(&mut buffer, r#"{"a":1}"#).unwrap();
        write(&mut buffer, r#"{"b":2}"#).unwrap();
        let mut input = Cursor::new(buffer);
        assert_eq!(
            read(&mut input).unwrap().as_deref(),
            Some(&b"{\"a\":1}"[..])
        );
        assert_eq!(
            read(&mut input).unwrap().as_deref(),
            Some(&b"{\"b\":2}"[..])
        );
    }

    #[test]
    fn the_length_is_counted_in_bytes_so_a_message_may_hold_any_text() {
        let body = r#"{"text":"ünïcode 🙂"}"#;
        let mut buffer: Vec<u8> = Vec::new();
        write(&mut buffer, body).unwrap();
        let mut input = Cursor::new(buffer);
        let got = read(&mut input).unwrap().unwrap();
        assert_eq!(String::from_utf8(got).unwrap(), body);
    }

    #[test]
    fn a_reader_that_ignores_the_header_gets_an_error_not_a_message() {
        // content-length is what servers send; Content-Length is what everyone means.
        let mut input = Cursor::new(b"content-length: 2\r\n\r\n{}".to_vec());
        assert_eq!(read(&mut input).unwrap().as_deref(), Some(&b"{}"[..]));
    }

    #[test]
    fn a_message_without_a_length_is_an_error() {
        let mut input = Cursor::new(b"Content-Type: application/json\r\n\r\n{}".to_vec());
        assert!(read(&mut input).is_err());
    }
}
