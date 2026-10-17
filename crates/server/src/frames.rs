//! Binary frame messages for the WebSocket and image encoding.

use mx_core::Frame;

/// `"MXF1"` little endian.
pub const FRAME_MAGIC: u32 = u32::from_le_bytes(*b"MXF1");
pub const FRAME_HEADER_LEN: usize = 32;

pub const FORMAT_RGB: u32 = 0;
pub const FORMAT_JPEG: u32 = 1;

pub const FLAG_EXACT: u32 = 1;
pub const FLAG_ORIGINAL: u32 = 2;

/// Header of a binary frame message (eight little-endian `u32`s), followed by the payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    pub seq: u32,
    pub index: u32,
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub flags: u32,
    pub rev: u32,
}

impl FrameHeader {
    pub fn encode(&self, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(FRAME_HEADER_LEN + payload.len());
        for v in [
            FRAME_MAGIC,
            self.seq,
            self.index,
            self.width,
            self.height,
            self.format,
            self.flags,
            self.rev,
        ] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(payload);
        out
    }

    pub fn decode(msg: &[u8]) -> Option<(FrameHeader, &[u8])> {
        if msg.len() < FRAME_HEADER_LEN {
            return None;
        }
        let word =
            |i: usize| u32::from_le_bytes(msg[i * 4..i * 4 + 4].try_into().expect("4 bytes"));
        if word(0) != FRAME_MAGIC {
            return None;
        }
        let header = FrameHeader {
            seq: word(1),
            index: word(2),
            width: word(3),
            height: word(4),
            format: word(5),
            flags: word(6),
            rev: word(7),
        };
        Some((header, &msg[FRAME_HEADER_LEN..]))
    }
}

pub fn png(frame: &Frame) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, frame.width(), frame.height());
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut writer = enc.write_header().expect("png header");
        writer.write_image_data(frame.data()).expect("png data");
    }
    out
}

pub fn jpeg(frame: &Frame, quality: u8) -> Vec<u8> {
    let mut out = Vec::new();
    let enc = jpeg_encoder::Encoder::new(&mut out, quality);
    enc.encode(
        frame.data(),
        frame.width() as u16,
        frame.height() as u16,
        jpeg_encoder::ColorType::Rgb,
    )
    .expect("jpeg encoding");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_roundtrip() {
        let h = FrameHeader {
            seq: 7,
            index: 42,
            width: 640,
            height: 360,
            format: FORMAT_RGB,
            flags: FLAG_EXACT,
            rev: 3,
        };
        let msg = h.encode(&[1, 2, 3]);
        assert_eq!(&msg[..4], b"MXF1");
        let (back, payload) = FrameHeader::decode(&msg).unwrap();
        assert_eq!(back, h);
        assert_eq!(payload, &[1, 2, 3]);
        assert!(FrameHeader::decode(&msg[..10]).is_none());
    }

    #[test]
    fn images_encode() {
        let f = Frame::filled(8, 4, [255, 0, 0]);
        assert_eq!(&png(&f)[1..4], b"PNG");
        assert_eq!(&jpeg(&f, 80)[..2], &[0xFF, 0xD8]);
    }
}
