//! JPEG and TIFF pictures read into pixels with the `image` crate (#414), so the interactive
//! mode can make their thumbnails itself rather than wait for the Mac app's.

use std::io::Cursor;

use image::{ImageFormat, ImageReader, Limits};

use super::png::{MOST_PIXELS, Pixels};

/// The pixels of the JPEG or TIFF in `bytes`, or `None` when it's neither, can't be read, or
/// has more pixels than a PNG may have to be read (64 Mi).
#[must_use]
pub fn decode_jpeg_or_tiff(bytes: &[u8]) -> Option<Pixels> {
    let format = image::guess_format(bytes).ok()?;
    if !matches!(format, ImageFormat::Jpeg | ImageFormat::Tiff) {
        return None;
    }
    let reader = |limits: Limits| {
        let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
        reader.limits(limits);
        reader
    };
    let mut limits = Limits::default();
    // Room for the picture at up to 16 bits a channel, and its 8-bit copy.
    limits.max_alloc = Some(MOST_PIXELS * 4 * 3);
    let (width, height) = reader(limits.clone()).into_dimensions().ok()?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MOST_PIXELS {
        return None;
    }
    let rgba = reader(limits).decode().ok()?.into_rgba8();
    Some(Pixels {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}
