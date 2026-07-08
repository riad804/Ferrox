//! Subtitle rendering: composite active cues onto an RGBA [`Frame`] using the
//! [`FontManager`], with fill, stroke, drop-shadow, background box, and
//! karaoke word-highlighting.

use crate::error::Result;
use crate::font::{FontManager, Glyph};
use crate::frame::Frame;

use super::model::{Cue, Subtitle};

/// Visual styling for rendered subtitles. Colors are straight-alpha `[r,g,b,a]`.
#[derive(Debug, Clone, PartialEq)]
pub struct SubtitleStyle {
    pub font_family: String,
    pub size: f32,
    /// Normal text fill.
    pub color: [u8; 4],
    /// Color for already-sung karaoke characters (ignored without `\k` timing).
    pub highlight_color: [u8; 4],
    /// Outline color + thickness in pixels (0 disables).
    pub stroke_color: [u8; 4],
    pub stroke_width: u32,
    /// Drop shadow `(color, dx, dy)`; `None` disables.
    pub shadow: Option<([u8; 4], i32, i32)>,
    /// Filled box behind the text (with a small pad); `None` disables.
    pub background: Option<[u8; 4]>,
    /// Gap in pixels between the bottom of the text and the frame bottom.
    pub margin_bottom: u32,
}

impl Default for SubtitleStyle {
    fn default() -> Self {
        Self {
            font_family: "default".into(),
            size: 32.0,
            color: [255, 255, 255, 255],
            highlight_color: [255, 210, 40, 255],
            stroke_color: [0, 0, 0, 255],
            stroke_width: 2,
            shadow: Some(([0, 0, 0, 160], 2, 2)),
            background: None,
            margin_bottom: 24,
        }
    }
}

/// Renders subtitle cues onto frames using a shared [`FontManager`].
pub struct SubtitleRenderer<'a> {
    fonts: &'a FontManager,
}

impl<'a> SubtitleRenderer<'a> {
    pub fn new(fonts: &'a FontManager) -> Self {
        Self { fonts }
    }

    /// Draw every cue active at time `t` onto `frame` (RGBA8), bottom-centered.
    pub fn render(&self, frame: &mut Frame, subs: &Subtitle, t: f64, style: &SubtitleStyle) -> Result<()> {
        let active = subs.active_cues(t);
        // Stack multiple simultaneous cues upward from the bottom margin.
        let line_h = (style.size * 1.25).ceil() as i32;
        for (row, cue) in active.iter().rev().enumerate() {
            let baseline_margin = style.margin_bottom as i32 + row as i32 * line_h;
            self.render_cue(frame, cue, t, style, baseline_margin)?;
        }
        Ok(())
    }

    fn render_cue(&self, frame: &mut Frame, cue: &Cue, t: f64, style: &SubtitleStyle, margin: i32) -> Result<()> {
        let laid = self.layout(&cue.text, style)?;
        if laid.glyphs.is_empty() {
            return Ok(());
        }
        // Center horizontally; place the text row above the bottom margin.
        let ox = (frame.width as i32 - laid.width) / 2;
        let baseline = frame.height as i32 - margin - (style.size * 0.25).ceil() as i32;
        let top = baseline - laid.ascent;

        if let Some(bg) = style.background {
            let pad = 6i32;
            fill_rect(frame, ox - pad, top - pad, laid.width + 2 * pad, laid.ascent + laid.descent + 2 * pad, bg);
        }
        if let Some((sc, dx, dy)) = style.shadow {
            self.draw_run(frame, &laid, Pen { ox: ox + dx, baseline: baseline + dy }, |_| sc, 0, style.stroke_color);
        }
        // Karaoke: characters before `hi` use the highlight color.
        let hi = cue.highlighted_chars(t);
        let (fill, hl) = (style.color, style.highlight_color);
        self.draw_run(frame, &laid, Pen { ox, baseline }, |i| if i < hi { hl } else { fill }, style.stroke_width, style.stroke_color);
        Ok(())
    }

    /// Layout: rasterize each char, accumulate advances, track ascent/descent.
    fn layout(&self, text: &str, style: &SubtitleStyle) -> Result<LaidText> {
        let mut glyphs = Vec::new();
        let mut pen = 0.0f32;
        let (mut ascent, mut descent) = (0i32, 0i32);
        for ch in text.chars() {
            if ch == '\n' {
                continue; // single-line rows; multi-line handled by cue stacking
            }
            let g = self.fonts.rasterize_glyph(&style.font_family, ch, style.size)?;
            ascent = ascent.max(g.top.max(0));
            descent = descent.max((g.height as i32 - g.top).max(0));
            glyphs.push(PlacedGlyph { glyph: g, x: pen });
            pen += glyphs.last().unwrap().glyph.advance;
        }
        Ok(LaidText { width: pen.ceil() as i32, ascent, descent, glyphs })
    }

    /// Draw all glyphs at a pen origin, coloring per-index; optional stroke ring.
    fn draw_run(&self, frame: &mut Frame, laid: &LaidText, pen: Pen, color: impl Fn(usize) -> [u8; 4], stroke: u32, stroke_color: [u8; 4]) {
        for (i, pg) in laid.glyphs.iter().enumerate() {
            let gx = pen.ox + pg.x.round() as i32 + pg.glyph.left;
            let gy = pen.baseline - pg.glyph.top;
            if stroke > 0 {
                let s = stroke as i32;
                for dy in -s..=s {
                    for dx in -s..=s {
                        if dx * dx + dy * dy <= s * s {
                            blit_glyph(frame, &pg.glyph, gx + dx, gy + dy, stroke_color);
                        }
                    }
                }
            }
            blit_glyph(frame, &pg.glyph, gx, gy, color(i));
        }
    }
}

/// A pen origin: left edge `ox` and text `baseline` in frame pixels.
#[derive(Clone, Copy)]
struct Pen {
    ox: i32,
    baseline: i32,
}

struct PlacedGlyph {
    glyph: std::sync::Arc<Glyph>,
    x: f32,
}

struct LaidText {
    width: i32,
    ascent: i32,
    descent: i32,
    glyphs: Vec<PlacedGlyph>,
}

/// Alpha-blend a glyph's coverage bitmap onto the frame at `(x, y)` (top-left).
fn blit_glyph(frame: &mut Frame, g: &Glyph, x: i32, y: i32, color: [u8; 4]) {
    for gy in 0..g.height as i32 {
        for gx in 0..g.width as i32 {
            let cov = g.coverage[(gy * g.width as i32 + gx) as usize];
            if cov == 0 {
                continue;
            }
            let a = (cov as u16 * color[3] as u16 / 255) as u8;
            blend_pixel(frame, x + gx, y + gy, [color[0], color[1], color[2], a]);
        }
    }
}

/// Fill an axis-aligned rectangle (clipped to the frame) with a color.
fn fill_rect(frame: &mut Frame, x: i32, y: i32, w: i32, h: i32, color: [u8; 4]) {
    for py in y..y + h {
        for px in x..x + w {
            blend_pixel(frame, px, py, color);
        }
    }
}

/// Source-over blend of a straight-alpha color onto one RGBA8 pixel.
fn blend_pixel(frame: &mut Frame, x: i32, y: i32, src: [u8; 4]) {
    if x < 0 || y < 0 || x >= frame.width as i32 || y >= frame.height as i32 {
        return;
    }
    let sa = src[3] as u32;
    if sa == 0 {
        return;
    }
    let i = ((y as u32 * frame.width + x as u32) * 4) as usize;
    for (c, &s) in src.iter().enumerate().take(3) {
        let d = frame.data[i + c] as u32;
        frame.data[i + c] = ((s as u32 * sa + d * (255 - sa)) / 255) as u8;
    }
    let da = frame.data[i + 3] as u32;
    frame.data[i + 3] = (sa + da * (255 - sa) / 255) as u8;
}
