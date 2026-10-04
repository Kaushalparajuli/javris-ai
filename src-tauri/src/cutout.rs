//! Cut-outs from an object sheet: a picture of several separate objects on one plain, flat background
//! (white, or any single colour) becomes one transparent PNG per object, ready to place in a video.
//!
//! No model is involved. The background is found by flooding in from the edges of the picture. An area
//! enclosed by an object stays part of it (the off-white of a plate) unless it is exactly the
//! background colour and big enough to be a hole (the middle of a ring or a doughnut), which becomes
//! transparent. Objects are the connected pieces left over; pieces that nearly touch (a cup and its
//! handle) are kept together, judged by the real distance between their pixels, not their boxes.

use image::{ImageBuffer, Rgba, RgbaImage};
use std::path::Path;

/// How far from the background colour a pixel may be and still count as background.
const TOLERANCE: f32 = 34.0;
/// Objects smaller than this share of the sheet are dust, not objects.
const MIN_SHARE: f32 = 0.0015;
/// Pieces closer than this many pixels belong to the same object.
const MERGE_GAP: usize = 14;
/// An enclosed area this close to the background colour, and at least this many pixels, is a hole.
const HOLE_TOLERANCE: f32 = 9.0;
const MIN_HOLE: usize = 400;
const PADDING: u32 = 8;
const MAX_OBJECTS: usize = 24;

fn dist(a: [u8; 3], b: [u8; 3]) -> f32 {
    let d = |i: usize| a[i] as f32 - b[i] as f32;
    (d(0) * d(0) + d(1) * d(1) + d(2) * d(2)).sqrt()
}

/// The sheet's background colour: the most common colour around its border.
fn background_of(img: &RgbaImage) -> [u8; 3] {
    let (w, h) = img.dimensions();
    let mut counts: std::collections::HashMap<[u8; 3], u32> = Default::default();
    let mut add = |x: u32, y: u32| {
        let p = img.get_pixel(x, y).0;
        // Quantise so tiny noise in a flat colour still counts as one colour.
        *counts.entry([p[0] & 0xF8, p[1] & 0xF8, p[2] & 0xF8]).or_default() += 1;
    };
    for x in 0..w {
        add(x, 0);
        add(x, h - 1);
    }
    for y in 0..h {
        add(0, y);
        add(w - 1, y);
    }
    let best = counts.into_iter().max_by_key(|(_, n)| *n).map(|(c, _)| c).unwrap_or([255, 255, 255]);
    [best[0] | 4, best[1] | 4, best[2] | 4]
}

/// Mark as outside the enclosed areas that are flat background colour: the middle of a ring.
fn punch_holes(img: &RgbaImage, bg: [u8; 3], outside: &mut [bool]) {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let flat = |i: usize| {
        let p = &img.as_raw()[i * 4..i * 4 + 4];
        p[3] >= 16 && dist([p[0], p[1], p[2]], bg) <= HOLE_TOLERANCE
    };
    let mut seen = vec![false; w * h];
    for start in 0..w * h {
        if outside[start] || seen[start] || !flat(start) {
            continue;
        }
        let mut region = vec![start];
        seen[start] = true;
        let mut k = 0;
        while k < region.len() {
            let i = region[k];
            k += 1;
            let (x, y) = (i % w, i / w);
            let mut next = |j: usize| {
                if !outside[j] && !seen[j] && flat(j) {
                    seen[j] = true;
                    region.push(j);
                }
            };
            if x > 0 {
                next(i - 1);
            }
            if x + 1 < w {
                next(i + 1);
            }
            if y > 0 {
                next(i - w);
            }
            if y + 1 < h {
                next(i + w);
            }
        }
        if region.len() >= MIN_HOLE {
            for i in region {
                outside[i] = true;
            }
        }
    }
}

/// `true` for pixels that are background connected to the picture's edge.
fn outside_mask(img: &RgbaImage, bg: [u8; 3]) -> Vec<bool> {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let mut out = vec![false; w * h];
    let mut stack: Vec<usize> = vec![];
    let is_bg = |i: usize| {
        let p = img.as_raw()[i * 4..i * 4 + 4].to_vec();
        p[3] < 16 || dist([p[0], p[1], p[2]], bg) <= TOLERANCE
    };
    let push = |i: usize, out: &mut Vec<bool>, stack: &mut Vec<usize>| {
        if !out[i] && is_bg(i) {
            out[i] = true;
            stack.push(i);
        }
    };
    for x in 0..w {
        push(x, &mut out, &mut stack);
        push((h - 1) * w + x, &mut out, &mut stack);
    }
    for y in 0..h {
        push(y * w, &mut out, &mut stack);
        push(y * w + w - 1, &mut out, &mut stack);
    }
    while let Some(i) = stack.pop() {
        let (x, y) = (i % w, i / w);
        if x > 0 {
            push(i - 1, &mut out, &mut stack);
        }
        if x + 1 < w {
            push(i + 1, &mut out, &mut stack);
        }
        if y > 0 {
            push(i - w, &mut out, &mut stack);
        }
        if y + 1 < h {
            push(i + w, &mut out, &mut stack);
        }
    }
    out
}

struct Piece {
    min: (i64, i64),
    max: (i64, i64),
    pixels: usize,
}

/// Connected pieces of everything that is not outside background, with their boxes.
fn pieces(outside: &[bool], w: usize, h: usize) -> (Vec<Piece>, Vec<u32>) {
    let mut label = vec![0u32; w * h];
    let mut found: Vec<Piece> = vec![];
    for start in 0..w * h {
        if outside[start] || label[start] != 0 {
            continue;
        }
        let id = found.len() as u32 + 1;
        let mut piece = Piece { min: (i64::MAX, i64::MAX), max: (0, 0), pixels: 0 };
        let mut stack = vec![start];
        label[start] = id;
        while let Some(i) = stack.pop() {
            let (x, y) = ((i % w) as i64, (i / w) as i64);
            piece.min = (piece.min.0.min(x), piece.min.1.min(y));
            piece.max = (piece.max.0.max(x), piece.max.1.max(y));
            piece.pixels += 1;
            for (dx, dy) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1), (-1, -1), (1, -1), (-1, 1), (1, 1)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                    continue;
                }
                let j = ny as usize * w + nx as usize;
                if !outside[j] && label[j] == 0 {
                    label[j] = id;
                    stack.push(j);
                }
            }
        }
        found.push(piece);
    }
    (found, label)
}

/// Group pieces that come within MERGE_GAP pixels of each other, by growing every object pixel outward
/// and seeing which pieces run together. Returns, for each group, its box and the piece labels in it.
fn group(found: &[Piece], label: &[u32], w: usize, h: usize, min_pixels: usize) -> Vec<((i64, i64, i64, i64), Vec<u32>)> {
    let big: Vec<u32> = (0..found.len()).filter(|&i| found[i].pixels >= min_pixels).map(|i| i as u32 + 1).collect();
    if big.is_empty() {
        return vec![];
    }
    let is_big = |l: u32| l != 0 && big.contains(&l);
    // Grow by r pixels, in two passes (across, then down), remembering which piece each pixel came from.
    let r = MERGE_GAP / 2;
    let mut owner: Vec<u32> = label.iter().map(|&l| if is_big(l) { l } else { 0 }).collect();
    let grow = |owner: &mut Vec<u32>, horizontal: bool| {
        let src = owner.clone();
        let (outer, inner) = if horizontal { (h, w) } else { (w, h) };
        for o in 0..outer {
            let at = |n: usize| if horizontal { o * w + n } else { n * w + o };
            for n in 0..inner {
                if src[at(n)] != 0 {
                    continue;
                }
                let lo = n.saturating_sub(r);
                let hi = (n + r).min(inner - 1);
                if let Some(l) = (lo..=hi).map(|m| src[at(m)]).find(|&l| l != 0) {
                    owner[at(n)] = l;
                }
            }
        }
    };
    grow(&mut owner, true);
    grow(&mut owner, false);
    // Pieces whose grown areas touch belong together.
    let index = |l: u32| big.iter().position(|&b| b == l).unwrap();
    let mut parent: Vec<usize> = (0..big.len()).collect();
    fn root(p: &mut Vec<usize>, i: usize) -> usize {
        let mut r = i;
        while p[r] != r {
            r = p[r];
        }
        let mut c = i;
        while p[c] != r {
            let n = p[c];
            p[c] = r;
            c = n;
        }
        r
    }
    for i in 0..w * h {
        let a = owner[i];
        if a == 0 {
            continue;
        }
        for j in [if i % w + 1 < w { Some(i + 1) } else { None }, if i / w + 1 < h { Some(i + w) } else { None }].into_iter().flatten() {
            let b = owner[j];
            if b != 0 && b != a {
                let (ra, rb) = (root(&mut parent, index(a)), root(&mut parent, index(b)));
                parent[ra] = rb;
            }
        }
    }
    let mut groups: std::collections::BTreeMap<usize, ((i64, i64, i64, i64), Vec<u32>)> = Default::default();
    for (k, &l) in big.iter().enumerate() {
        let r = root(&mut parent, k);
        let p = &found[l as usize - 1];
        let e = groups.entry(r).or_insert(((i64::MAX, i64::MAX, 0, 0), vec![]));
        e.0 = (e.0 .0.min(p.min.0), e.0 .1.min(p.min.1), e.0 .2.max(p.max.0), e.0 .3.max(p.max.1));
        e.1.push(l);
    }
    groups.into_values().collect()
}

/// Split the sheet at `sheet` into `<id>-1.png`, `<id>-2.png`… in `out_dir`, in reading order.
/// Returns the file names.
pub fn split_sheet(sheet: &Path, out_dir: &Path, id: &str) -> Result<Vec<String>, String> {
    let img = image::open(sheet).map_err(|e| format!("Couldn't read the sheet: {e}"))?.to_rgba8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    if w < 16 || h < 16 {
        return Err("The sheet is too small.".into());
    }
    let bg = background_of(&img);
    let mut outside = outside_mask(&img, bg);
    punch_holes(&img, bg, &mut outside);
    let (found, label) = pieces(&outside, w, h);
    let min_pixels = ((w * h) as f32 * MIN_SHARE) as usize;
    let mut groups = group(&found, &label, w, h, min_pixels.max(24));
    if groups.is_empty() {
        return Ok(vec![]);
    }
    // Reading order: rows of objects (by the middle of each box), left to right inside a row.
    let row_h = groups.iter().map(|g| g.0 .3 - g.0 .1).max().unwrap_or(1).max(1) / 2;
    groups.sort_by_key(|g| ((g.0 .1 + g.0 .3) / 2 / row_h.max(1), g.0 .0));
    groups.truncate(MAX_OBJECTS);
    std::fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
    let mut names = vec![];
    for (n, (bx, labels)) in groups.iter().enumerate() {
        let (x0, y0) = ((bx.0 - PADDING as i64).max(0) as u32, (bx.1 - PADDING as i64).max(0) as u32);
        let (x1, y1) = ((bx.2 + PADDING as i64).min(w as i64 - 1) as u32, (bx.3 + PADDING as i64).min(h as i64 - 1) as u32);
        let mut out: RgbaImage = ImageBuffer::from_pixel(x1 - x0 + 1, y1 - y0 + 1, Rgba([0, 0, 0, 0]));
        for y in y0..=y1 {
            for x in x0..=x1 {
                let i = y as usize * w + x as usize;
                if labels.contains(&label[i]) && label[i] != 0 {
                    let mut p = *img.get_pixel(x, y);
                    // The rim of an object blends into the background: fade it by how far it is from the background colour.
                    let near_edge = [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)].iter().any(|(dx, dy)| {
                        let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                        nx >= 0 && ny >= 0 && nx < w as i64 && ny < h as i64 && outside[ny as usize * w + nx as usize]
                    });
                    if near_edge {
                        let d = dist([p[0], p[1], p[2]], bg);
                        p[3] = (((d - TOLERANCE * 0.5) / (TOLERANCE * 1.5)).clamp(0.15, 1.0) * 255.0) as u8;
                    }
                    out.put_pixel(x - x0, y - y0, p);
                }
            }
        }
        let name = format!("{id}-{}.png", n + 1);
        out.save(out_dir.join(&name)).map_err(|e| format!("Couldn't save {name}: {e}"))?;
        names.push(name);
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(img: &mut RgbaImage, x0: u32, y0: u32, x1: u32, y1: u32, c: [u8; 3]) {
        for y in y0..y1 {
            for x in x0..x1 {
                img.put_pixel(x, y, Rgba([c[0], c[1], c[2], 255]));
            }
        }
    }

    #[test]
    fn a_sheet_becomes_one_transparent_picture_per_object() {
        let dir = std::env::temp_dir().join(format!("jarvis-cutout-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut sheet: RgbaImage = ImageBuffer::from_pixel(400, 200, Rgba([255, 255, 255, 255]));
        rect(&mut sheet, 20, 30, 90, 100, [200, 30, 30]); // a red square
        // A blue ring whose middle is off-white, like the rim of a plate: that is part of the object.
        rect(&mut sheet, 160, 40, 260, 140, [30, 60, 200]);
        rect(&mut sheet, 185, 65, 235, 115, [244, 244, 236]);
        rect(&mut sheet, 300, 20, 380, 80, [30, 160, 60]); // a green block, with a crumb of dust nearby
        rect(&mut sheet, 396, 196, 398, 198, [0, 0, 0]);
        let path = dir.join("sheet.png");
        sheet.save(&path).unwrap();

        let names = split_sheet(&path, &dir, "bread").unwrap();
        assert_eq!(names, vec!["bread-1.png", "bread-2.png", "bread-3.png"], "three objects, left to right, no dust");
        let ring = image::open(dir.join("bread-2.png")).unwrap().to_rgba8();
        assert_eq!(ring.get_pixel(0, 0).0[3], 0, "the corner outside the object is transparent");
        assert_eq!(ring.get_pixel(ring.width() / 2, ring.height() / 2).0, [244, 244, 236, 255], "off-white inside an object stays solid");
        assert_eq!(ring.get_pixel(PADDING + 3, ring.height() / 2).0[3], 255, "the ring itself is solid");
        let red = image::open(dir.join("bread-1.png")).unwrap().to_rgba8();
        assert_eq!((red.width(), red.height()), (70 + 2 * PADDING, 70 + 2 * PADDING), "cropped tight with a small margin");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_hole_of_a_ring_is_transparent_and_objects_whose_boxes_overlap_stay_apart() {
        let dir = std::env::temp_dir().join(format!("jarvis-cutout3-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut sheet: RgbaImage = ImageBuffer::from_pixel(400, 300, Rgba([255, 255, 255, 255]));
        // A doughnut: a ring with a hole that is exactly the sheet's white.
        rect(&mut sheet, 20, 20, 140, 140, [200, 120, 40]);
        rect(&mut sheet, 50, 50, 110, 110, [255, 255, 255]);
        // A wide loaf below it: its top edge is only 10px under the doughnut's bottom edge, and
        // their boxes overlap sideways, but the shapes are far apart where it counts.
        rect(&mut sheet, 20, 150, 380, 250, [150, 90, 40]);
        // Make the corner of the doughnut and the loaf not touch: cut the loaf's top-left away.
        rect(&mut sheet, 20, 150, 200, 200, [255, 255, 255]);
        let path = dir.join("sheet.png");
        sheet.save(&path).unwrap();
        let names = split_sheet(&path, &dir, "d").unwrap();
        assert_eq!(names.len(), 2, "the doughnut and the loaf are two objects: {names:?}");
        let ring = image::open(dir.join("d-1.png")).unwrap().to_rgba8();
        assert_eq!(ring.get_pixel(ring.width() / 2, ring.height() / 2).0[3], 0, "the hole in the middle is transparent");
        assert_eq!(ring.get_pixel(PADDING + 5, ring.height() / 2).0[3], 255, "the doughnut itself is solid");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The sheet a real image model produced for the bakery reel: sel roti ring, lakhamari and a loaf.
    /// `COUTOUT_SHEET=<path> cargo test -- --ignored --nocapture a_real_generated_sheet_gives_three_objects`
    #[test]
    #[ignore]
    fn a_real_generated_sheet_gives_three_objects() {
        let sheet = std::path::PathBuf::from(std::env::var("CUTOUT_SHEET").expect("set CUTOUT_SHEET to a generated sheet"));
        let out = std::env::temp_dir().join(format!("jarvis-cutout-real-{}", std::process::id()));
        let names = split_sheet(&sheet, &out, "obj").unwrap();
        println!("{} objects in {}", names.len(), out.display());
        for n in &names {
            let im = image::open(out.join(n)).unwrap().to_rgba8();
            let transparent = im.pixels().filter(|p| p.0[3] == 0).count() as f32 / (im.width() * im.height()) as f32;
            println!("  {n}: {}x{}, {:.0}% transparent", im.width(), im.height(), transparent * 100.0);
        }
        assert_eq!(names.len(), 3);
    }

    #[test]
    fn any_flat_colour_works_as_the_background_and_close_pieces_stay_together() {
        let dir = std::env::temp_dir().join(format!("jarvis-cutout2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut sheet: RgbaImage = ImageBuffer::from_pixel(300, 150, Rgba([20, 200, 90, 255])); // green screen
        rect(&mut sheet, 30, 40, 100, 110, [240, 240, 240]); // a cup...
        rect(&mut sheet, 108, 60, 130, 90, [240, 240, 240]); // ...and its handle, 8px away
        rect(&mut sheet, 200, 40, 270, 110, [90, 40, 20]);
        let path = dir.join("sheet.png");
        sheet.save(&path).unwrap();
        let names = split_sheet(&path, &dir, "x").unwrap();
        assert_eq!(names.len(), 2, "the handle belongs to the cup");
        let cup = image::open(dir.join("x-1.png")).unwrap();
        assert_eq!(cup.width(), 100 + 2 * PADDING, "the cup's picture includes its handle (30 to 130)");
        assert!(split_sheet(&dir.join("missing.png"), &dir, "x").is_err());
        let blank = dir.join("blank.png");
        ImageBuffer::from_pixel(100, 100, Rgba([255u8, 255, 255, 255])).save(&blank).unwrap();
        assert!(split_sheet(&blank, &dir, "b").unwrap().is_empty(), "an empty sheet has no objects");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
