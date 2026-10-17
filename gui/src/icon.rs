//! The app icon: a rounded blue square with a white play symbol. Shared with
//! build.rs, which turns it into the .ico embedded in the exe.

/// RGBA pixels (top row first) of an `n`×`n` icon, 4×4 supersampled.
pub fn rgba(n: u32) -> Vec<u8> {
    const SS: u32 = 4;
    // drawn in a 64×64 design space
    let scale = 64.0 / n as f32;
    let inside_square = |x: f32, y: f32| {
        let (lo, hi, r) = (3.0, 61.0, 14.0);
        let cx = x.clamp(lo + r, hi - r);
        let cy = y.clamp(lo + r, hi - r);
        (x - cx).powi(2) + (y - cy).powi(2) <= r * r && (lo..=hi).contains(&x) && (lo..=hi).contains(&y)
    };
    let inside_play = |x: f32, y: f32| {
        let edge = |ax: f32, ay: f32, bx: f32, by: f32| (bx - ax) * (y - ay) - (by - ay) * (x - ax);
        edge(24.0, 18.0, 47.0, 32.0) >= 0.0
            && edge(47.0, 32.0, 24.0, 46.0) >= 0.0
            && edge(24.0, 46.0, 24.0, 18.0) >= 0.0
    };
    let mut out = Vec::with_capacity((n * n * 4) as usize);
    for py in 0..n {
        for px in 0..n {
            let (mut cover, mut white) = (0.0f32, 0.0f32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = (px as f32 + (sx as f32 + 0.5) / SS as f32) * scale;
                    let y = (py as f32 + (sy as f32 + 0.5) / SS as f32) * scale;
                    if inside_square(x, y) {
                        cover += 1.0;
                        if inside_play(x, y) {
                            white += 1.0;
                        }
                    }
                }
            }
            // vertical gradient from light to deeper blue
            let t = py as f32 / n as f32;
            let base = [lerp(0x5c, 0x2e, t), lerp(0x9c, 0x5e, t), lerp(0xff, 0xdc, t)];
            let w = if cover > 0.0 { white / cover } else { 0.0 };
            for c in base {
                out.push((c + (255.0 - c) * w).round() as u8);
            }
            out.push((255.0 * cover / (SS * SS) as f32).round() as u8);
        }
    }
    out
}

fn lerp(a: u8, b: u8, t: f32) -> f32 {
    a as f32 + (b as f32 - a as f32) * t
}
