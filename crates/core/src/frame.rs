use std::fmt;

/// An 8-bit RGB image with tightly packed rows (`width * 3` bytes per row).
#[derive(Clone, PartialEq, Eq)]
pub struct Frame {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FrameError {
    #[error("frame data has {actual} bytes, expected {expected} for {width}x{height} RGB")]
    BadLength {
        width: u32,
        height: u32,
        expected: usize,
        actual: usize,
    },
}

impl Frame {
    /// Number of bytes of an RGB frame of the given size.
    pub fn byte_len(width: u32, height: u32) -> usize {
        width as usize * height as usize * 3
    }

    pub fn new(width: u32, height: u32, data: Vec<u8>) -> Result<Self, FrameError> {
        let expected = Self::byte_len(width, height);
        if data.len() != expected {
            return Err(FrameError::BadLength {
                width,
                height,
                expected,
                actual: data.len(),
            });
        }
        Ok(Self {
            width,
            height,
            data,
        })
    }

    pub fn filled(width: u32, height: u32, rgb: [u8; 3]) -> Self {
        let data = rgb
            .iter()
            .copied()
            .cycle()
            .take(Self::byte_len(width, height))
            .collect();
        Self {
            width,
            height,
            data,
        }
    }

    pub fn black(width: u32, height: u32) -> Self {
        Self::filled(width, height, [0, 0, 0])
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn data_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }

    pub fn into_data(self) -> Vec<u8> {
        self.data
    }

    fn offset(&self, x: u32, y: u32) -> usize {
        assert!(x < self.width && y < self.height, "pixel out of bounds");
        (y as usize * self.width as usize + x as usize) * 3
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 3] {
        let i = self.offset(x, y);
        [self.data[i], self.data[i + 1], self.data[i + 2]]
    }

    pub fn set_pixel(&mut self, x: u32, y: u32, rgb: [u8; 3]) {
        let i = self.offset(x, y);
        self.data[i..i + 3].copy_from_slice(&rgb);
    }

    /// Fills the axis-aligned rectangle `[x, x+w) × [y, y+h)` (clipped to the frame).
    pub fn fill_rect(&mut self, x: u32, y: u32, w: u32, h: u32, rgb: [u8; 3]) {
        for yy in y..(y + h).min(self.height) {
            for xx in x..(x + w).min(self.width) {
                self.set_pixel(xx, yy, rgb);
            }
        }
    }
}

impl fmt::Debug for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Frame({}x{})", self.width, self.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_checks_length() {
        assert!(Frame::new(2, 2, vec![0; 12]).is_ok());
        assert_eq!(
            Frame::new(2, 2, vec![0; 11]),
            Err(FrameError::BadLength {
                width: 2,
                height: 2,
                expected: 12,
                actual: 11
            })
        );
    }

    #[test]
    fn pixels_and_rects() {
        let mut f = Frame::filled(4, 3, [1, 2, 3]);
        assert_eq!(f.pixel(3, 2), [1, 2, 3]);
        f.fill_rect(2, 1, 10, 10, [9, 9, 9]);
        assert_eq!(f.pixel(1, 1), [1, 2, 3]);
        assert_eq!(f.pixel(2, 1), [9, 9, 9]);
        assert_eq!(f.pixel(3, 2), [9, 9, 9]);
    }
}
