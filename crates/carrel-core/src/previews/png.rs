//! Just enough PNG for previews: reading the common kinds of PNG into pixels, shrinking them,
//! and writing them back, so a terminal can be sent a thumbnail rather than a whole picture,
//! and a Sixel terminal can be sent pixels at all.
//!
//! Reads 8- and 16-bit grey, grey with alpha, RGB and RGBA, and 1-, 2-, 4- and 8-bit palette
//! and grey images, without interlacing. Anything else reads as `None`, and its caller shows
//! no picture rather than a wrong one.

use std::io::{Read as _, Write as _};

/// The signature every PNG starts with.
const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// The largest picture read, in pixels, so a huge or hostile file can't take all the memory.
const MOST_PIXELS: u64 = 64 * 1024 * 1024;

/// A picture as 8-bit RGBA pixels, row by row from the top left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pixels {
    /// Its width in pixels.
    pub width: u32,
    /// Its height in pixels.
    pub height: u32,
    /// Four bytes a pixel: red, green, blue and alpha.
    pub rgba: Vec<u8>,
}

/// A PNG's width and height, from its header, without reading the rest.
#[must_use]
pub fn size(bytes: &[u8]) -> Option<(u32, u32)> {
    if !bytes.starts_with(&SIGNATURE) || bytes.get(12..16) != Some(b"IHDR") {
        return None;
    }
    Some((be32(bytes.get(16..20)?), be32(bytes.get(20..24)?)))
}

fn be32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

/// The parts of a PNG that decoding needs.
struct Parts {
    width: u32,
    height: u32,
    depth: u8,
    colour: u8,
    palette: Vec<[u8; 3]>,
    transparency: Vec<u8>,
    data: Vec<u8>,
}

fn parts(bytes: &[u8]) -> Option<Parts> {
    if !bytes.starts_with(&SIGNATURE) {
        return None;
    }
    let mut at = SIGNATURE.len();
    let mut found: Option<Parts> = None;
    while at + 8 <= bytes.len() {
        let length = usize::try_from(be32(&bytes[at..])).ok()?;
        let kind = &bytes[at + 4..at + 8];
        let body = bytes.get(at + 8..at + 8 + length)?;
        at += 12 + length;
        match kind {
            b"IHDR" if body.len() >= 13 => {
                // Compression and filter methods are always 0; interlacing isn't read.
                if body[10] != 0 || body[11] != 0 || body[12] != 0 {
                    return None;
                }
                found = Some(Parts {
                    width: be32(body),
                    height: be32(&body[4..]),
                    depth: body[8],
                    colour: body[9],
                    palette: Vec::new(),
                    transparency: Vec::new(),
                    data: Vec::new(),
                });
            }
            b"PLTE" => {
                found.as_mut()?.palette =
                    body.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
            }
            b"tRNS" => found.as_mut()?.transparency = body.to_vec(),
            b"IDAT" => found.as_mut()?.data.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
    }
    found
}

/// The pixels of the PNG in `bytes`, or `None` when it isn't one this reads.
#[must_use]
pub fn decode(bytes: &[u8]) -> Option<Pixels> {
    let parts = parts(bytes)?;
    let channels: usize = match (parts.colour, parts.depth) {
        (0 | 3, 1 | 2 | 4 | 8) | (0, 16) => 1,
        (2, 8 | 16) => 3,
        (4, 8 | 16) => 2,
        (6, 8 | 16) => 4,
        _ => return None,
    };
    if parts.width == 0
        || parts.height == 0
        || u64::from(parts.width) * u64::from(parts.height) > MOST_PIXELS
    {
        return None;
    }
    let width = usize::try_from(parts.width).ok()?;
    let height = usize::try_from(parts.height).ok()?;
    let bits = channels * usize::from(parts.depth);
    let stride = (width * bits).div_ceil(8);
    // Filters look this many bytes to the left: a whole pixel, or one byte below 8 bits.
    let step = bits.div_ceil(8);
    let mut raw = Vec::with_capacity((stride + 1) * height);
    flate2::read::ZlibDecoder::new(parts.data.as_slice())
        .take(u64::try_from((stride + 1) * height).ok()?)
        .read_to_end(&mut raw)
        .ok()?;
    if raw.len() < (stride + 1) * height {
        return None;
    }
    let mut rows = vec![0u8; stride * height];
    for y in 0..height {
        let line = &raw[y * (stride + 1)..(y + 1) * (stride + 1)];
        let (before, now) = rows.split_at_mut(y * stride);
        let above = (y > 0).then(|| &before[(y - 1) * stride..]);
        unfilter(line[0], &line[1..], &mut now[..stride], above, step)?;
    }
    let mut rgba = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        let row = &rows[y * stride..(y + 1) * stride];
        for x in 0..width {
            rgba.extend_from_slice(&pixel(&parts, row, x, channels));
        }
    }
    Some(Pixels {
        width: parts.width,
        height: parts.height,
        rgba,
    })
}

/// Undoes one row's filter, into `out`.
fn unfilter(
    filter: u8,
    line: &[u8],
    out: &mut [u8],
    above: Option<&[u8]>,
    step: usize,
) -> Option<()> {
    for i in 0..line.len() {
        let a = if i >= step { out[i - step] } else { 0 };
        let b = above.map_or(0, |r| r[i]);
        let c = match above {
            Some(r) if i >= step => r[i - step],
            _ => 0,
        };
        out[i] = line[i].wrapping_add(match filter {
            0 => 0,
            1 => a,
            2 => b,
            3 => u8::try_from((u16::from(a) + u16::from(b)) / 2).unwrap_or(0),
            4 => paeth(a, b, c),
            _ => return None,
        });
    }
    Some(())
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let (pa, pb, pc) = (
        (p - i16::from(a)).abs(),
        (p - i16::from(b)).abs(),
        (p - i16::from(c)).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Sample `index` of a row, scaled to 8 bits.
fn sample(parts: &Parts, row: &[u8], index: usize) -> u8 {
    match parts.depth {
        16 => row[index * 2],
        8 => row[index],
        depth => {
            let depth = usize::from(depth);
            let bit = index * depth;
            let byte = row[bit / 8];
            let shift = 8 - depth - bit % 8;
            let value = (byte >> shift) & ((1u8 << depth) - 1);
            if parts.colour == 3 {
                value
            } else {
                // Grey below 8 bits is stretched to the whole range.
                value * (255 / ((1u8 << depth) - 1))
            }
        }
    }
}

fn pixel(parts: &Parts, row: &[u8], x: usize, channels: usize) -> [u8; 4] {
    let s = |c: usize| sample(parts, row, x * channels + c);
    match parts.colour {
        0 => [s(0), s(0), s(0), 255],
        2 => [s(0), s(1), s(2), 255],
        3 => {
            let index = usize::from(s(0));
            let [r, g, b] = parts.palette.get(index).copied().unwrap_or_default();
            [
                r,
                g,
                b,
                parts.transparency.get(index).copied().unwrap_or(255),
            ]
        }
        4 => [s(0), s(0), s(0), s(1)],
        _ => [s(0), s(1), s(2), s(3)],
    }
}

impl Pixels {
    /// This picture shrunk, keeping its shape, to fit `width` by `height` pixels; itself when
    /// it fits already. Each new pixel is the average of those it covers.
    #[must_use]
    pub fn fit(&self, width: u32, height: u32) -> Self {
        if self.width <= width && self.height <= height {
            return self.clone();
        }
        let scale = (f64::from(width) / f64::from(self.width))
            .min(f64::from(height) / f64::from(self.height));
        let to_w = shrunk(self.width, scale);
        let to_h = shrunk(self.height, scale);
        let (w, h) = (self.width as usize, self.height as usize);
        let mut rgba = Vec::with_capacity(to_w as usize * to_h as usize * 4);
        for ty in 0..to_h as usize {
            let (y0, y1) = span(ty, h, to_h as usize);
            for tx in 0..to_w as usize {
                let (x0, x1) = span(tx, w, to_w as usize);
                let mut sum = [0u64; 4];
                for y in y0..y1 {
                    for x in x0..x1 {
                        let p = &self.rgba[(y * w + x) * 4..(y * w + x) * 4 + 4];
                        for (s, v) in sum.iter_mut().zip(p) {
                            *s += u64::from(*v);
                        }
                    }
                }
                let count = ((y1 - y0) * (x1 - x0)) as u64;
                rgba.extend(sum.map(|s| u8::try_from(s / count).unwrap_or(u8::MAX)));
            }
        }
        Self {
            width: to_w,
            height: to_h,
            rgba,
        }
    }

    /// This picture as a PNG: RGBA, unfiltered, compressed.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let width = self.width as usize;
        let mut rows = Vec::with_capacity((width * 4 + 1) * self.height as usize);
        for row in self.rgba.chunks_exact(width * 4) {
            rows.push(0);
            rows.extend_from_slice(row);
        }
        let mut deflated =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        let _ = deflated.write_all(&rows);
        let data = deflated.finish().unwrap_or_default();
        let mut header = Vec::with_capacity(13);
        header.extend_from_slice(&self.width.to_be_bytes());
        header.extend_from_slice(&self.height.to_be_bytes());
        header.extend_from_slice(&[8, 6, 0, 0, 0]);
        let mut png = SIGNATURE.to_vec();
        chunk(&mut png, b"IHDR", &header);
        chunk(&mut png, b"IDAT", &data);
        chunk(&mut png, b"IEND", &[]);
        png
    }
}

/// `length` scaled by `scale`, at least one pixel.
fn shrunk(length: u32, scale: f64) -> u32 {
    // Pictures are at most MOST_PIXELS, so this fits.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let scaled = (f64::from(length) * scale).round() as u32;
    scaled.clamp(1, length)
}

/// The source pixels new pixel `i` of `to` covers, of `from`.
fn span(i: usize, from: usize, to: usize) -> (usize, usize) {
    let start = i * from / to;
    let end = ((i + 1) * from / to).max(start + 1).min(from);
    (start, end)
}

fn chunk(png: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    png.extend_from_slice(&u32::try_from(body.len()).unwrap_or(0).to_be_bytes());
    png.extend_from_slice(kind);
    png.extend_from_slice(body);
    png.extend_from_slice(&crc(kind.iter().chain(body)).to_be_bytes());
}

/// The CRC-32 a PNG chunk ends with, of its type and body.
fn crc<'a>(bytes: impl Iterator<Item = &'a u8>) -> u32 {
    !bytes.fold(!0u32, |crc, byte| {
        (0..8).fold(crc ^ u32::from(*byte), |c, _| {
            if c & 1 == 1 {
                (c >> 1) ^ 0xedb8_8320
            } else {
                c >> 1
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checks(width: u32, height: u32) -> Pixels {
        let mut rgba = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let v = if (x + y) % 2 == 0 { 255 } else { 0 };
                rgba.extend_from_slice(&[v, 0, 255 - v, 255]);
            }
        }
        Pixels {
            width,
            height,
            rgba,
        }
    }

    #[test]
    fn pictures_written_read_back_the_same() {
        let picture = checks(7, 5);
        let png = picture.encode();
        assert_eq!(size(&png), Some((7, 5)));
        assert_eq!(decode(&png), Some(picture));
        assert_eq!(decode(b"not a png"), None);
        assert_eq!(size(b"GIF89a"), None);
    }

    #[test]
    fn filtered_and_paletted_rows_are_read() {
        // A 2×2 palette picture, 1 bit a pixel, its second row filtered "up".
        let mut rows = vec![0, 0b1000_0000, 2, 0b1100_0000];
        let mut deflated =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        deflated.write_all(&rows).unwrap();
        rows = deflated.finish().unwrap();
        let mut png = SIGNATURE.to_vec();
        chunk(&mut png, b"IHDR", &[0, 0, 0, 2, 0, 0, 0, 2, 1, 3, 0, 0, 0]);
        chunk(&mut png, b"PLTE", &[10, 20, 30, 200, 210, 220]);
        chunk(&mut png, b"tRNS", &[0]);
        chunk(&mut png, b"IDAT", &rows);
        chunk(&mut png, b"IEND", &[]);
        let pixels = decode(&png).unwrap();
        // Row two adds row one's byte: 0b1100_0000 + 0b1000_0000 wraps to 0b0100_0000.
        assert_eq!(
            pixels.rgba,
            [
                200, 210, 220, 255, 10, 20, 30, 0, //
                10, 20, 30, 0, 200, 210, 220, 255,
            ]
        );
    }

    #[test]
    fn pictures_shrink_to_fit_keeping_their_shape() {
        let picture = checks(400, 100);
        let small = picture.fit(100, 100);
        assert_eq!((small.width, small.height), (100, 25));
        assert_eq!(small.rgba.len(), 100 * 25 * 4);
        // Averaged: the checks become an even purple.
        assert_eq!(&small.rgba[..4], &[127, 0, 127, 255]);
        assert_eq!(picture.fit(500, 500), picture);
    }
}
