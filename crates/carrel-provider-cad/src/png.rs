//! Pictures as PNG files: 8-bit RGB, each row unfiltered, compressed with zlib.

use std::io::Write as _;

use crate::render::Image;

/// `image` as a PNG file's bytes.
#[must_use]
pub fn encode(image: &Image) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&u32::try_from(image.width).unwrap_or(u32::MAX).to_be_bytes());
    header.extend_from_slice(
        &u32::try_from(image.height)
            .unwrap_or(u32::MAX)
            .to_be_bytes(),
    );
    // 8 bits, RGB, deflate, adaptive filtering, not interlaced.
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, *b"IHDR", &header);
    let mut rows = Vec::with_capacity((image.width * 3 + 1) * image.height);
    for row in image.pixels.chunks_exact(image.width * 3) {
        rows.push(0); // no filter
        rows.extend_from_slice(row);
    }
    let mut zlib = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    zlib.write_all(&rows)
        .expect("writing to memory doesn't fail");
    chunk(
        &mut out,
        *b"IDAT",
        &zlib.finish().expect("writing to memory doesn't fail"),
    );
    chunk(&mut out, *b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) {
    out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(u32::MAX).to_be_bytes());
    out.extend_from_slice(&kind);
    out.extend_from_slice(data);
    let mut crc = crc32fast::Hasher::new();
    crc.update(&kind);
    crc.update(data);
    out.extend_from_slice(&crc.finalize().to_be_bytes());
}

#[cfg(test)]
mod tests {
    use std::io::Read as _;

    use super::*;

    #[test]
    fn pngs_have_their_header_and_pixels() {
        let image = Image {
            width: 2,
            height: 1,
            pixels: vec![255, 0, 0, 0, 0, 255],
        };
        let png = encode(&image);
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR\0\0\0\x02\0\0\0\x01\x08\x02"));
        assert!(
            png.ends_with(b"IEND\xae\x42\x60\x82"),
            "IEND's CRC is well known"
        );
        let start = png.windows(4).position(|w| w == b"IDAT").unwrap() + 4;
        let length = u32::from_be_bytes(png[start - 8..start - 4].try_into().unwrap()) as usize;
        let mut rows = Vec::new();
        flate2::read::ZlibDecoder::new(&png[start..start + length])
            .read_to_end(&mut rows)
            .unwrap();
        assert_eq!(rows, [0, 255, 0, 0, 0, 0, 255]);
    }
}
