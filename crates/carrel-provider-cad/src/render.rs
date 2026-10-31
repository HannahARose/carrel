//! A small software renderer: a model drawn from four sides, angled, front, top and side, in
//! light grey on white with flat shading and a depth buffer, as one 2 × 2 picture. It needs
//! no GPU, so it draws the same everywhere. Z is up, as in most CAD and print files.

/// Each view's side, in pixels; the four make one picture twice as wide and high.
pub const TILE: usize = 512;

/// Each view is drawn this many times larger, then scaled down, to smooth its edges.
const SAMPLES: usize = 2;

/// An RGB picture.
#[derive(Debug, Clone)]
pub struct Image {
    /// Its width and height, in pixels.
    pub width: usize,
    /// Its height, in pixels.
    pub height: usize,
    /// Three bytes a pixel, row by row from the top.
    pub pixels: Vec<u8>,
}

type Vector = [f32; 3];

/// From where each view looks, and which way is up in it.
const VIEWS: [(Vector, Vector); 4] = [
    ([1.0, -1.0, 0.8], [0.0, 0.0, 1.0]), // angled
    ([0.0, -1.0, 0.0], [0.0, 0.0, 1.0]), // front
    ([0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),  // top
    ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),  // side
];

/// The four views of a model with these corners and triangles. None when it has no size.
#[must_use]
pub fn views(positions: &[Vector], triangles: &[[u32; 3]]) -> Option<Image> {
    let (low, high) = bounds(positions)?;
    let center = [0, 1, 2].map(|i| f32::midpoint(low[i], high[i]));
    // The bounding sphere fits in each view, whichever way it looks.
    let radius = length(sub(high, low)) / 2.0;
    if radius <= 0.0 || !radius.is_finite() {
        return None;
    }
    let side = TILE * 2;
    let mut image = Image {
        width: side,
        height: side,
        pixels: vec![255; side * side * 3],
    };
    for (index, (from, up)) in VIEWS.iter().enumerate() {
        let tile = draw(positions, triangles, center, radius, *from, *up);
        let (left, top) = (index % 2 * TILE, index / 2 * TILE);
        for y in 0..TILE {
            let row = ((top + y) * side + left) * 3;
            image.pixels[row..row + TILE * 3]
                .copy_from_slice(&tile[y * TILE * 3..(y + 1) * TILE * 3]);
        }
    }
    Some(image)
}

/// One view, `TILE` pixels a side, as RGB.
fn draw(
    positions: &[Vector],
    triangles: &[[u32; 3]],
    center: Vector,
    radius: f32,
    from: Vector,
    up: Vector,
) -> Vec<u8> {
    let size = TILE * SAMPLES;
    // The view's axes: toward the viewer, to the right, and up.
    let back = normalize(from);
    let right = normalize(cross(up, back));
    let upward = cross(back, right);
    #[allow(clippy::cast_precision_loss)] // a few thousand pixels
    let scale = size as f32 * 0.46 / radius;
    #[allow(clippy::cast_precision_loss)]
    let half = size as f32 / 2.0;
    // Light from over the viewer's left shoulder, so faces facing different ways differ.
    let light = normalize(add(add(back, scale_by(upward, 0.6)), scale_by(right, -0.4)));

    let projected: Vec<Vector> = positions
        .iter()
        .map(|p| {
            let d = sub(*p, center);
            [
                half + dot(d, right) * scale,
                half - dot(d, upward) * scale,
                dot(d, back),
            ]
        })
        .collect();
    let mut depth = vec![f32::NEG_INFINITY; size * size];
    let mut shade = vec![255u8; size * size];
    for triangle in triangles {
        let Some(corners) = triangle
            .iter()
            .map(|&i| positions.get(i as usize).zip(projected.get(i as usize)))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let normal = cross(
            sub(*corners[1].0, *corners[0].0),
            sub(*corners[2].0, *corners[0].0),
        );
        if length(normal) == 0.0 {
            continue;
        }
        // Both sides lit alike: files often have triangles wound the wrong way.
        let lit = dot(normalize(normal), light).abs();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // 0 to 255
        let grey = (110.0 + 120.0 * lit) as u8;
        fill(
            [*corners[0].1, *corners[1].1, *corners[2].1],
            grey,
            size,
            &mut depth,
            &mut shade,
        );
    }
    downsample(&shade, size)
}

/// Fills a projected triangle where it's nearer than what's drawn.
fn fill(corners: [Vector; 3], grey: u8, size: usize, depth: &mut [f32], shade: &mut [u8]) {
    let [a, b, c] = corners;
    let area = edge(a, b, c);
    if area.abs() < f32::EPSILON {
        return;
    }
    #[allow(clippy::cast_precision_loss)]
    let limit = size as f32 - 1.0;
    let min_x = a[0].min(b[0]).min(c[0]).floor().clamp(0.0, limit);
    let max_x = a[0].max(b[0]).max(c[0]).ceil().clamp(0.0, limit);
    let min_y = a[1].min(b[1]).min(c[1]).floor().clamp(0.0, limit);
    let max_y = a[1].max(b[1]).max(c[1]).ceil().clamp(0.0, limit);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped above
    let (x0, x1, y0, y1) = (
        min_x as usize,
        max_x as usize,
        min_y as usize,
        max_y as usize,
    );
    for y in y0..=y1 {
        for x in x0..=x1 {
            #[allow(clippy::cast_precision_loss)]
            let point = [x as f32 + 0.5, y as f32 + 0.5, 0.0];
            let (wa, wb, wc) = (
                edge(b, c, point) / area,
                edge(c, a, point) / area,
                edge(a, b, point) / area,
            );
            if wa < 0.0 || wb < 0.0 || wc < 0.0 {
                continue;
            }
            let z = wa * a[2] + wb * b[2] + wc * c[2];
            let at = y * size + x;
            if z > depth[at] {
                depth[at] = z;
                shade[at] = grey;
            }
        }
    }
}

/// Twice the signed area of the triangle `a`, `b`, `p`, in the picture's plane.
fn edge(a: Vector, b: Vector, p: Vector) -> f32 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

/// A grey picture `SAMPLES` times larger scaled down to `TILE`, as RGB.
fn downsample(shade: &[u8], size: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(TILE * TILE * 3);
    for y in 0..TILE {
        for x in 0..TILE {
            let mut sum = 0u32;
            for dy in 0..SAMPLES {
                for dx in 0..SAMPLES {
                    sum += u32::from(shade[(y * SAMPLES + dy) * size + x * SAMPLES + dx]);
                }
            }
            #[allow(clippy::cast_possible_truncation)] // an average of bytes
            let grey = (sum / (SAMPLES * SAMPLES) as u32) as u8;
            out.extend_from_slice(&[grey, grey, grey]);
        }
    }
    out
}

/// The smallest and largest corner, when there are corners and they're finite.
#[must_use]
pub fn bounds(positions: &[Vector]) -> Option<(Vector, Vector)> {
    let mut points = positions.iter().filter(|p| p.iter().all(|v| v.is_finite()));
    let first = *points.next()?;
    Some(points.fold((first, first), |(low, high), p| {
        (
            [0, 1, 2].map(|i| low[i].min(p[i])),
            [0, 1, 2].map(|i| high[i].max(p[i])),
        )
    }))
}

fn add(a: Vector, b: Vector) -> Vector {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: Vector, b: Vector) -> Vector {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale_by(a: Vector, s: f32) -> Vector {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn dot(a: Vector, b: Vector) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: Vector, b: Vector) -> Vector {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn length(a: Vector) -> f32 {
    dot(a, a).sqrt()
}

fn normalize(a: Vector) -> Vector {
    scale_by(a, 1.0 / length(a))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A box `x` by `y` by `z` from the origin.
    pub(crate) fn cube(x: f32, y: f32, z: f32) -> (Vec<Vector>, Vec<[u32; 3]>) {
        let positions = (0..8)
            .map(|i| {
                [
                    if i & 1 == 0 { 0.0 } else { x },
                    if i & 2 == 0 { 0.0 } else { y },
                    if i & 4 == 0 { 0.0 } else { z },
                ]
            })
            .collect();
        let triangles = vec![
            [0, 2, 1],
            [1, 2, 3],
            [4, 5, 6],
            [5, 7, 6],
            [0, 1, 4],
            [1, 5, 4],
            [2, 6, 3],
            [3, 6, 7],
            [0, 4, 2],
            [2, 4, 6],
            [1, 3, 5],
            [3, 7, 5],
        ];
        (positions, triangles)
    }

    fn grey(image: &Image, x: usize, y: usize) -> u8 {
        image.pixels[(y * image.width + x) * 3]
    }

    #[test]
    fn each_view_has_the_model_in_its_middle_on_white() {
        let (positions, triangles) = cube(86.0, 86.0, 19.0);
        let image = views(&positions, &triangles).unwrap();
        assert_eq!((image.width, image.height), (TILE * 2, TILE * 2));
        assert_eq!(grey(&image, 2, 2), 255);
        for (x, y) in [(1, 1), (3, 1), (1, 3), (3, 3)] {
            let middle = grey(&image, TILE * x / 2, TILE * y / 2);
            assert!((110..=230).contains(&middle), "view at {x}, {y}: {middle}");
        }
    }

    #[test]
    fn faces_facing_different_ways_are_shaded_differently() {
        let (positions, triangles) = cube(10.0, 10.0, 10.0);
        let image = views(&positions, &triangles).unwrap();
        // In the angled view the top, front and right faces all show; they mustn't be one grey.
        let shades: std::collections::BTreeSet<u8> = (TILE / 4..TILE * 3 / 4)
            .step_by(8)
            .flat_map(|y| (TILE / 4..TILE * 3 / 4).step_by(8).map(move |x| (x, y)))
            .map(|(x, y)| grey(&image, x, y))
            .filter(|&g| g < 250)
            .collect();
        assert!(shades.len() >= 3, "{shades:?}");
    }

    #[test]
    fn nothing_to_draw_is_none() {
        assert!(views(&[], &[]).is_none());
        assert!(views(&[[1.0, 1.0, 1.0]], &[]).is_none());
    }
}
