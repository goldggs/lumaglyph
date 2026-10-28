use rayon::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SamplePreset {
    CyberSculpture,
    SynthwaveHorizon,
    ChromaticSpheres,
}

impl SamplePreset {
    pub const ALL: [SamplePreset; 3] = [
        SamplePreset::CyberSculpture,
        SamplePreset::SynthwaveHorizon,
        SamplePreset::ChromaticSpheres,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SamplePreset::CyberSculpture => "Cyber Torus Sculpture",
            SamplePreset::SynthwaveHorizon => "Synthwave Solar Grid",
            SamplePreset::ChromaticSpheres => "Chromatic Glass Orbs",
        }
    }
}

/// Generate high-detail RGBA32 procedural showcase media in a few milliseconds with zero asset bloat.
/// Designed with rich curves, crisp silhouettes, diagonals, specular rims, and color gradients
/// to exercise all 95 printable ASCII shape vectors immediately on launch.
pub fn generate_sample_rgba(preset: SamplePreset, w: u32, h: u32, phase: f32) -> Vec<u8> {
    let mut buf = vec![0u8; (w as usize) * (h as usize) * 4];
    let wf = w as f32;
    let hf = h as f32;
    let aspect = wf / hf.max(1.0);

    buf.par_chunks_mut((w as usize) * 4)
        .enumerate()
        .for_each(|(py, row_bytes)| {
            let v = ((py as f32 + 0.5) / hf) * 2.0 - 1.0; // -1 top .. +1 bottom
            for px in 0..(w as usize) {
                let u = (((px as f32 + 0.5) / wf) * 2.0 - 1.0) * aspect;
                let (r, g, b) = match preset {
                    SamplePreset::CyberSculpture => shade_cyber_sculpture(u, -v, phase),
                    SamplePreset::SynthwaveHorizon => shade_synthwave(u, -v, phase),
                    SamplePreset::ChromaticSpheres => shade_glass_orbs(u, -v, phase),
                };
                let o = px * 4;
                row_bytes[o] = (r.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                row_bytes[o + 1] = (g.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                row_bytes[o + 2] = (b.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                row_bytes[o + 3] = 255;
            }
        });
    buf
}

#[inline]
fn rot2(x: f32, y: f32, a: f32) -> (f32, f32) {
    let (s, c) = a.sin_cos();
    (x * c - y * s, x * s + y * c)
}

#[inline]
fn sdf_cyber_scene(px: f32, py: f32, pz: f32, phase: f32) -> (f32, u8) {
    // Rotate main torus-knot / interlocking rings
    let (rx, rz) = rot2(px, pz, 0.55 + phase * 0.9);
    let (ry, rz2) = rot2(py, rz, 0.38 + (phase * 0.55).sin() * 0.35);

    // Primary sculpted twisted torus with fluted ridges
    let q_xy = (rx * rx + ry * ry).sqrt() - 0.68;
    let angle = ry.atan2(rx);
    let twist = (angle * 3.0 + phase * 1.5).sin() * 0.045;
    let d_torus = (q_xy * q_xy + rz2 * rz2).sqrt() - (0.23 + twist);

    // Perpendicular tilted halo ring for sharp diagonal intersections (`/` `\`)
    let (hx, hy) = rot2(px, py, -0.68);
    let (hy2, hz2) = rot2(hy, pz, 1.05 + phase * 0.7);
    let q_halo = (hx * hx + hy2 * hy2).sqrt() - 1.02;
    let d_halo = (q_halo * q_halo + hz2 * hz2).sqrt() - 0.065;

    // Floating faceted core octahedron/sphere hybrid
    let d_sphere = (px * px + py * py + pz * pz).sqrt() - 0.31;
    let d_oct = (px.abs() + py.abs() + pz.abs()) * 0.57735 - 0.30;
    let d_core = 0.45 * d_sphere + 0.55 * d_oct;

    let mut d = d_torus;
    let mut mat = 1u8;
    if d_halo < d {
        d = d_halo;
        mat = 2;
    }
    if d_core < d {
        d = d_core;
        mat = 3;
    }
    (d, mat)
}

fn shade_cyber_sculpture(u: f32, y: f32, phase: f32) -> (f32, f32, f32) {
    // Camera at (0, 0, 2.75) looking toward origin
    let ro = [0.0f32, 0.0, 2.75];
    let inv_len = 1.0 / (u * u + y * y + 1.85 * 1.85).sqrt();
    let rd = [u * inv_len, y * inv_len, -1.85 * inv_len];

    let mut t = 0.0f32;
    let mut hit_mat = 0u8;
    for _ in 0..42 {
        let p = [ro[0] + rd[0] * t, ro[1] + rd[1] * t, ro[2] + rd[2] * t];
        let (d, m) = sdf_cyber_scene(p[0], p[1], p[2], phase);
        if d < 0.0022 {
            hit_mat = m;
            break;
        }
        t += d;
        if t > 5.0 {
            break;
        }
    }

    if hit_mat == 0 {
        // Subtle architectural studio backdrop with faint radial grid arcs
        let rad = (u * u + y * y).sqrt();
        let ring = ((rad * 14.0).cos() * 0.5 + 0.5).powi(18) * 0.10;
        let diag = (((u + y) * 9.0).sin().abs() < 0.045) as i32 as f32 * 0.05;
        let glow = (0.14 / (0.22 + rad * rad)).min(0.28);
        return (
            0.02 + glow * 0.25 + diag,
            0.03 + glow * 0.52 + ring,
            0.06 + glow * 0.95 + ring + diag,
        );
    }

    let p = [ro[0] + rd[0] * t, ro[1] + rd[1] * t, ro[2] + rd[2] * t];
    let e = 0.0025f32;
    let nx = sdf_cyber_scene(p[0] + e, p[1], p[2], phase).0
        - sdf_cyber_scene(p[0] - e, p[1], p[2], phase).0;
    let ny = sdf_cyber_scene(p[0], p[1] + e, p[2], phase).0
        - sdf_cyber_scene(p[0], p[1] - e, p[2], phase).0;
    let nz = sdf_cyber_scene(p[0], p[1], p[2] + e, phase).0
        - sdf_cyber_scene(p[0], p[1], p[2] - e, phase).0;
    let n_len = (nx * nx + ny * ny + nz * nz).sqrt().max(1e-5);
    let (nx, ny, nz) = (nx / n_len, ny / n_len, nz / n_len);

    // Key light (warm amber-coral top-right) + Rim light (electric cyan left)
    let k_dir = [0.62f32, 0.58, 0.52];
    let diff_key = (nx * k_dir[0] + ny * k_dir[1] + nz * k_dir[2]).max(0.0);
    let r_dir = [-0.74f32, 0.28, 0.60];
    let diff_fill = (nx * r_dir[0] + ny * r_dir[1] + nz * r_dir[2]).max(0.0);

    // Fresnel rim + specular highlight
    let ndotv = (-(nx * rd[0] + ny * rd[1] + nz * rd[2])).clamp(0.0, 1.0);
    let fresnel = (1.0 - ndotv).powi(3);
    let hx = k_dir[0] - rd[0];
    let hy = k_dir[1] - rd[1];
    let hz = k_dir[2] - rd[2];
    let h_len = (hx * hx + hy * hy + hz * hz).sqrt().max(1e-5);
    let spec = ((nx * hx + ny * hy + nz * hz) / h_len).max(0.0).powi(28);

    let base = match hit_mat {
        1 => (0.16 + 0.55 * (ny * 0.5 + 0.5), 0.72, 0.96), // Iridescent cyan-azure torus
        2 => (1.00, 0.44, 0.16),                           // Warm solar-amber halo ring
        _ => (0.94, 0.20, 0.68),                           // Magenta-ruby faceted core
    };

    let r = base.0 * (0.14 + 0.88 * diff_key + 0.22 * diff_fill) + spec * 0.95 + fresnel * 0.35;
    let g = base.1 * (0.14 + 0.78 * diff_key + 0.48 * diff_fill) + spec * 0.90 + fresnel * 0.75;
    let b = base.2 * (0.16 + 0.68 * diff_key + 0.65 * diff_fill) + spec * 0.95 + fresnel * 0.95;
    (r, g, b)
}

fn shade_synthwave(u: f32, y: f32, phase: f32) -> (f32, f32, f32) {
    let horizon = -0.10f32;
    if y >= horizon {
        // Sky gradient + sliced retro sun + jagged mountain silhouettes
        let sky_t = ((y - horizon) / (1.0 - horizon)).clamp(0.0, 1.0);
        let mut r = 0.34 * (1.0 - sky_t) + 0.04;
        let mut g = 0.08 * (1.0 - sky_t) + 0.03;
        let mut b = 0.48 * (1.0 - sky_t) + 0.14;

        // Sliced solar disc
        let sx = u;
        let sy = y - 0.28;
        let sr = (sx * sx + sy * sy).sqrt();
        if sr < 0.48 {
            let band = ((sy + 0.48) * 19.0 - phase * 1.4).sin();
            let cut = sy > 0.05 || band > -0.25 + sy * 1.1;
            if cut {
                let st = ((sy + 0.48) / 0.96).clamp(0.0, 1.0);
                r = 1.0;
                g = 0.22 + 0.72 * st;
                b = 0.48 * (1.0 - st) + 0.10;
            }
        } else {
            // Sun halo
            let halo = (0.045 / (sr - 0.46).max(0.045)) * 0.32;
            r += halo * 1.0;
            g += halo * 0.35;
            b += halo * 0.65;
        }

        // Crisp mountain ridges (`/`, `\`, `^`)
        let mtn = -0.10
            + 0.24 * (1.0 - ((u + 0.65) * 1.8).abs()).max(0.0)
            + 0.29 * (1.0 - ((u - 0.58) * 1.6).abs()).max(0.0)
            + 0.14 * (1.0 - (u * 2.4).abs()).max(0.0);
        if y < mtn {
            let edge_dist = (mtn - y).abs();
            if edge_dist < 0.016 {
                return (0.22, 0.92, 1.0); // Neon cyan ridge rim
            }
            return (0.05, 0.04, 0.11);
        }

        // Crisp stars in upper sky
        let cell_x = ((u + 3.0) * 18.0).floor();
        let cell_y = ((y + 2.0) * 18.0).floor();
        let hash = ((cell_x * 127.1 + cell_y * 311.7).sin() * 43758.547)
            .fract()
            .abs();
        if hash > 0.965 && sr > 0.56 {
            let fx = ((u + 3.0) * 18.0).fract() - 0.5;
            let fy = ((y + 2.0) * 18.0).fract() - 0.5;
            if fx * fx + fy * fy < 0.08 {
                r = 0.92;
                g = 0.96;
                b = 1.0;
            }
        }
        (r, g, b)
    } else {
        // Perspective neon floor grid (`/` `\` `_` `+`) on deep obsidian floor
        let depth = (horizon - y).max(0.004);
        let z = 0.34 / depth + phase * 1.8;
        let x_world = (u / depth) * 0.44;

        let dist_x = (x_world - x_world.round()).abs();
        let dist_z = (z - z.round()).abs();
        let thick_x = 0.042 + depth * 0.025;
        let thick_z = 0.055 + depth * 0.035;
        let line_x = (1.0 - dist_x / thick_x).clamp(0.0, 1.0);
        let line_z = (1.0 - dist_z / thick_z).clamp(0.0, 1.0);
        let fade = (1.0 - depth * 0.55).clamp(0.18, 1.0);
        let grid = line_x.max(line_z) * fade;
        let horizon_glow = (-depth * 14.0).exp() * 0.42;

        (
            0.02 + grid * 0.28 + horizon_glow * 0.95,
            0.02 + grid * 0.90 + horizon_glow * 0.24,
            0.05 + grid * 1.00 + horizon_glow * 0.72,
        )
    }
}

fn shade_glass_orbs(u: f32, y: f32, phase: f32) -> (f32, f32, f32) {
    let spheres: [([f32; 3], f32, [f32; 3]); 3] = [
        (
            [-0.58 + (phase * 0.7).cos() * 0.08, 0.08, 0.0],
            0.52,
            [0.18, 0.88, 1.00],
        ),
        (
            [0.48, -0.05 + (phase * 0.9).sin() * 0.08, 0.18],
            0.46,
            [1.00, 0.42, 0.24],
        ),
        ([-0.04, 0.42, -0.35], 0.34, [0.72, 0.34, 1.00]),
    ];

    let ro = [0.0f32, 0.0, 2.4];
    let inv = 1.0 / (u * u + y * y + 1.8 * 1.8).sqrt();
    let rd = [u * inv, y * inv, -1.8 * inv];

    let mut best_t = f32::MAX;
    let mut hit_idx = None;
    for (i, (c, rad, _)) in spheres.iter().enumerate() {
        let oc = [ro[0] - c[0], ro[1] - c[1], ro[2] - c[2]];
        let b = oc[0] * rd[0] + oc[1] * rd[1] + oc[2] * rd[2];
        let c_val = oc[0] * oc[0] + oc[1] * oc[1] + oc[2] * oc[2] - rad * rad;
        let disc = b * b - c_val;
        if disc > 0.0 {
            let t = -b - disc.sqrt();
            if t > 0.01 && t < best_t {
                best_t = t;
                hit_idx = Some(i);
            }
        }
    }

    if let Some(i) = hit_idx {
        let (c, rad, col) = spheres[i];
        let hit = [
            ro[0] + rd[0] * best_t,
            ro[1] + rd[1] * best_t,
            ro[2] + rd[2] * best_t,
        ];
        let n = [
            (hit[0] - c[0]) / rad,
            (hit[1] - c[1]) / rad,
            (hit[2] - c[2]) / rad,
        ];
        let l = [0.58f32, 0.64, 0.50];
        let diff = (n[0] * l[0] + n[1] * l[1] + n[2] * l[2]).max(0.0);
        let ndotv = (-(n[0] * rd[0] + n[1] * rd[1] + n[2] * rd[2])).clamp(0.0, 1.0);
        let rim = (1.0 - ndotv).powi(3);
        let spec = (n[0] * 0.35 + n[1] * 0.45 + n[2] * 0.82).max(0.0).powi(36);
        let caustic = (1.0 - rim) * (diff * 0.85 + 0.18);
        (
            col[0] * caustic + rim * col[0] * 0.9 + spec,
            col[1] * caustic + rim * col[1] * 0.9 + spec,
            col[2] * caustic + rim * col[2] * 0.9 + spec,
        )
    } else {
        // Subtle dark checkered studio floor bounce
        let checker = (((u * 3.5).floor() as i32 + (y * 3.5).floor() as i32) & 1) as f32;
        let vignette = (1.0 - (u * u + y * y) * 0.25).clamp(0.1, 1.0);
        let base = (0.04 + checker * 0.035) * vignette;
        (base * 0.85, base * 0.95, base * 1.25)
    }
}
