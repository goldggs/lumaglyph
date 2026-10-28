use crate::color::{active_canvas_bg, ColorMode};
use crate::engine::{rasterize_rgba, AsciiFrame, RenderParams};
use crate::glyph_atlas::GlyphAtlas;
use image::{ImageBuffer, RgbaImage};
use std::fmt::Write as FmtWrite;
use std::fs;
use std::path::Path;

/// Export `AsciiFrame` as a crisp PNG image (supports True OLED `#000000` and true RGBA transparency).
pub fn export_png(
    frame: &AsciiFrame,
    base_atlas: &GlyphAtlas,
    scale_cell_h: usize,
    params: &RenderParams,
    path: &Path,
) -> Result<(usize, usize), String> {
    let atlas = base_atlas.with_cell_height(scale_cell_h);
    let mut rgba = Vec::new();
    let (w, h) = rasterize_rgba(frame, &atlas, params, 0.0, &mut rgba);
    if w == 0 || h == 0 {
        return Err("Empty ASCII frame".to_string());
    }
    let img: RgbaImage = ImageBuffer::from_raw(w as u32, h as u32, rgba)
        .ok_or_else(|| "Failed to construct RGBA image buffer".to_string())?;
    img.save(path)
        .map_err(|e| format!("PNG save failed: {e}"))?;
    Ok((w, h))
}

/// Export `AsciiFrame` as plain 7-bit printable ASCII `.txt`.
pub fn export_txt(frame: &AsciiFrame, path: &Path) -> Result<(), String> {
    fs::write(path, frame.to_plain_text()).map_err(|e| format!("Failed to write TXT: {e}"))
}

/// Build a 24-bit TrueColor ANSI string (`\x1b[38;2;R;G;Bm` + optional `\x1b[48;2;R;G;Bm`).
pub fn frame_to_ansi_string(frame: &AsciiFrame, params: &RenderParams) -> String {
    let cols = frame.cols as usize;
    let rows = frame.rows as usize;
    let include_bg =
        (params.color_mode == ColorMode::DualColor || params.oled_black) && !params.transparent_bg;
    let mut out = String::with_capacity(cols * rows * 10);

    for r in 0..rows {
        let mut prev_fg: Option<[u8; 3]> = None;
        let mut prev_bg: Option<[u8; 3]> = None;
        let row_slice = &frame.cells[r * cols..(r + 1) * cols];

        for cell in row_slice {
            if include_bg && prev_bg != Some(cell.bg) {
                let _ = write!(
                    out,
                    "\x1b[48;2;{};{};{}m",
                    cell.bg[0], cell.bg[1], cell.bg[2]
                );
                prev_bg = Some(cell.bg);
            }
            if cell.ch != b' ' && prev_fg != Some(cell.fg) {
                let _ = write!(
                    out,
                    "\x1b[38;2;{};{};{}m",
                    cell.fg[0], cell.fg[1], cell.fg[2]
                );
                prev_fg = Some(cell.fg);
            }
            out.push(cell.ch as char);
        }
        out.push_str("\x1b[0m");
        if r + 1 < rows {
            out.push('\n');
        }
    }
    out
}

pub fn export_ansi(frame: &AsciiFrame, params: &RenderParams, path: &Path) -> Result<(), String> {
    let s = frame_to_ansi_string(frame, params);
    fs::write(path, s).map_err(|e| format!("Failed to write ANSI file: {e}"))
}

fn xml_escape_char(ch: u8, out: &mut String) {
    match ch {
        b'&' => out.push_str("&amp;"),
        b'<' => out.push_str("&lt;"),
        b'>' => out.push_str("&gt;"),
        b'"' => out.push_str("&quot;"),
        b'\'' => out.push_str("&#39;"),
        _ => out.push(ch as char),
    }
}

/// Export standalone scalable `.svg` (omits backdrop `<rect>` when `transparent_bg` is enabled).
pub fn export_svg(frame: &AsciiFrame, params: &RenderParams, path: &Path) -> Result<(), String> {
    let cols = frame.cols as usize;
    let rows = frame.rows as usize;
    let cw = 9.6f32;
    let ch = 16.0f32;
    let width = (cols as f32 * cw).ceil() as u32 + 24;
    let height = (rows as f32 * ch).ceil() as u32 + 24;
    let base_bg = active_canvas_bg(params.oled_black);
    let include_bg_cells = params.color_mode == ColorMode::DualColor && !params.transparent_bg;

    let mut svg = String::with_capacity(cols * rows * 16 + 1024);
    let _ = writeln!(
        svg,
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" width="{width}" height="{height}">"##
    );
    if !params.transparent_bg {
        let _ = writeln!(
            svg,
            r##"<rect width="100%" height="100%" fill="#{0:02X}{1:02X}{2:02X}"/>"##,
            base_bg[0], base_bg[1], base_bg[2]
        );
    }

    if include_bg_cells {
        svg.push_str("<g shape-rendering=\"crispEdges\">\n");
        for r in 0..rows {
            let row_slice = &frame.cells[r * cols..(r + 1) * cols];
            let mut c = 0;
            while c < cols {
                let bg = row_slice[c].bg;
                let start = c;
                c += 1;
                while c < cols && row_slice[c].bg == bg {
                    c += 1;
                }
                if bg != base_bg {
                    let x = 12.0 + start as f32 * cw;
                    let y = 12.0 + r as f32 * ch;
                    let w = (c - start) as f32 * cw;
                    let _ = writeln!(
                        svg,
                        r##"<rect x="{x:.1}" y="{y:.1}" width="{w:.1}" height="{ch:.1}" fill="#{0:02X}{1:02X}{2:02X}"/>"##,
                        bg[0], bg[1], bg[2]
                    );
                }
            }
        }
        svg.push_str("</g>\n");
    }

    svg.push_str(
        r##"<g font-family="'JetBrains Mono','Cascadia Mono','Fira Code',Consolas,monospace" font-size="13.2" xml:space="preserve">"##,
    );
    svg.push('\n');

    for r in 0..rows {
        let y = 12.0 + (r as f32 + 0.80) * ch;
        let _ = write!(svg, r##"<text x="12" y="{y:.1}">"##);
        let row_slice = &frame.cells[r * cols..(r + 1) * cols];
        let mut c = 0;
        while c < cols {
            let fg = row_slice[c].fg;
            let _ = write!(
                svg,
                r##"<tspan fill="#{0:02X}{1:02X}{2:02X}">"##,
                fg[0], fg[1], fg[2]
            );
            xml_escape_char(row_slice[c].ch, &mut svg);
            c += 1;
            while c < cols && (row_slice[c].fg == fg || row_slice[c].ch == b' ') {
                xml_escape_char(row_slice[c].ch, &mut svg);
                c += 1;
            }
            svg.push_str("</tspan>");
        }
        svg.push_str("</text>\n");
    }

    svg.push_str("</g>\n</svg>\n");
    fs::write(path, svg).map_err(|e| format!("Failed to write SVG: {e}"))
}

/// Export standalone dark-themed `.html` document (supports OLED `#000000` and transparent body).
pub fn export_html(frame: &AsciiFrame, params: &RenderParams, path: &Path) -> Result<(), String> {
    let cols = frame.cols as usize;
    let rows = frame.rows as usize;
    let base_bg = active_canvas_bg(params.oled_black);
    let include_bg = params.color_mode == ColorMode::DualColor && !params.transparent_bg;
    let bg_css = if params.transparent_bg {
        "transparent".to_string()
    } else {
        format!("#{:02X}{:02X}{:02X}", base_bg[0], base_bg[1], base_bg[2])
    };
    let mut html = String::with_capacity(cols * rows * 18 + 1024);

    let _ = writeln!(
        html,
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>LumaGlyph ASCII Export</title>
<style>
  body {{
    margin: 0;
    padding: 24px;
    background: {bg_css};
    color: #e5e9f0;
    display: flex;
    justify-content: center;
  }}
  pre {{
    margin: 0;
    font-family: 'JetBrains Mono', 'Cascadia Mono', 'Fira Code', Consolas, monospace;
    font-size: 11px;
    line-height: 1.15;
    letter-spacing: 0;
    user-select: text;
  }}
</style>
</head>
<body>
<pre>"#
    );

    for r in 0..rows {
        let row_slice = &frame.cells[r * cols..(r + 1) * cols];
        let mut c = 0;
        while c < cols {
            let fg = row_slice[c].fg;
            let bg = row_slice[c].bg;
            if include_bg && bg != base_bg {
                let _ = write!(
                    html,
                    r#"<span style="color:#{:02X}{:02X}{:02X};background:#{:02X}{:02X}{:02X}">"#,
                    fg[0], fg[1], fg[2], bg[0], bg[1], bg[2]
                );
            } else {
                let _ = write!(
                    html,
                    r#"<span style="color:#{:02X}{:02X}{:02X}">"#,
                    fg[0], fg[1], fg[2]
                );
            }
            xml_escape_char(row_slice[c].ch, &mut html);
            c += 1;
            while c < cols && row_slice[c].fg == fg && (!include_bg || row_slice[c].bg == bg) {
                xml_escape_char(row_slice[c].ch, &mut html);
                c += 1;
            }
            html.push_str("</span>");
        }
        if r + 1 < rows {
            html.push('\n');
        }
    }

    html.push_str("</pre>\n</body>\n</html>\n");
    fs::write(path, html).map_err(|e| format!("Failed to write HTML: {e}"))
}
