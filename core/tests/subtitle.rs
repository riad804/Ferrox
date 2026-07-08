//! Phase 11 subtitle engine: parse SRT / WebVTT / ASS (with `\k` karaoke),
//! query active cues + karaoke progress, and (with a font) render structurally.

use ferrox_core::subtitle::{Subtitle, SubtitleFormat};

const SRT: &str = "1\n00:00:01,000 --> 00:00:02,500\nHello world\n\n\
2\n00:00:03,000 --> 00:00:04,000\nSecond line\nwrapped\n";

#[test]
fn parses_srt_timing_and_text() {
    let s = Subtitle::from_srt(SRT).unwrap();
    assert_eq!(s.len(), 2);
    assert_eq!(s.cues[0].start, 1.0);
    assert_eq!(s.cues[0].end, 2.5);
    assert_eq!(s.cues[0].text, "Hello world");
    assert_eq!(s.cues[1].text, "Second line\nwrapped");
}

#[test]
fn active_cues_respect_bounds() {
    let s = Subtitle::from_srt(SRT).unwrap();
    assert_eq!(s.active_cues(0.5).len(), 0);
    assert_eq!(s.active_cues(1.0).len(), 1, "inclusive at start");
    assert_eq!(s.active_cues(2.5).len(), 0, "exclusive at end");
    assert_eq!(s.active_cues(3.5).len(), 1);
}

#[test]
fn parses_webvtt_and_strips_tags() {
    let vtt = "WEBVTT\n\n00:00:01.000 --> 00:00:02.000 align:middle\n\
Hello <c.yellow>styled</c> text\n";
    let s = Subtitle::from_vtt(vtt).unwrap();
    assert_eq!(s.len(), 1);
    assert_eq!(s.cues[0].start, 1.0);
    assert_eq!(s.cues[0].text, "Hello styled text");
}

#[test]
fn parses_webvtt_short_timestamp() {
    let vtt = "WEBVTT\n\n01:02.500 --> 01:04.000\nMinutes only\n";
    let s = Subtitle::from_vtt(vtt).unwrap();
    assert_eq!(s.cues[0].start, 62.5);
}

#[test]
fn parses_ass_dialogue_and_karaoke() {
    let ass = "[Script Info]\nTitle: t\n\n[Events]\n\
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:01.00,0:00:03.00,Default,,0,0,0,,{\\k50}Ka{\\k50}ra{\\k100}o{\\k50}ke\n";
    let s = Subtitle::from_ass(ass).unwrap();
    assert_eq!(s.len(), 1);
    let cue = &s.cues[0];
    assert_eq!(cue.start, 1.0);
    assert_eq!(cue.end, 3.0);
    assert_eq!(cue.text, "Karaoke", "override tags stripped, syllables joined");
    assert_eq!(cue.segments.len(), 4);
    assert_eq!(cue.segments[0].text, "Ka");
    assert_eq!(cue.segments[0].start, 0.0);
    assert_eq!(cue.segments[0].duration, 0.5);
    assert_eq!(cue.segments[2].text, "o");
    assert_eq!(cue.segments[2].start, 1.0); // 0.5 + 0.5
    assert_eq!(cue.segments[2].duration, 1.0);
}

#[test]
fn karaoke_progress_advances_over_time() {
    let ass = "[Events]\n\
Format: Start, End, Text\n\
Dialogue: 0:00:00.00,0:00:02.00,{\\k50}Ka{\\k50}ra{\\k50}o{\\k50}ke\n";
    let s = Subtitle::from_ass(ass).unwrap();
    let cue = &s.cues[0];
    assert_eq!(cue.text, "Karaoke");
    assert_eq!(cue.highlighted_chars(0.0), 0, "nothing sung yet");
    // After the first syllable (0.5s), its 2 chars are fully highlighted.
    assert_eq!(cue.highlighted_chars(0.5), 2);
    // Halfway through the last syllable, all prior + partial.
    assert!(cue.highlighted_chars(1.75) >= 6);
    assert_eq!(cue.highlighted_chars(2.0), 7, "all chars at the end");
}

#[test]
fn plain_cue_has_no_karaoke_highlight() {
    let s = Subtitle::from_srt("1\n00:00:00,000 --> 00:00:05,000\nplain\n").unwrap();
    assert_eq!(s.cues[0].highlighted_chars(2.5), 0);
    assert!(s.cues[0].segments.is_empty());
}

#[test]
fn generic_parse_dispatches_by_format() {
    assert_eq!(
        Subtitle::parse(SRT, SubtitleFormat::Srt).unwrap().len(),
        2
    );
}

#[test]
fn serde_round_trip_preserves_cues() {
    let s = Subtitle::from_ass(
        "[Events]\nFormat: Start, End, Text\nDialogue: 0:00:00.00,0:00:01.00,{\\k100}hi\n",
    )
    .unwrap();
    let json = serde_json::to_string(&s).unwrap();
    let back: Subtitle = serde_json::from_str(&json).unwrap();
    assert_eq!(s, back);
}

// ── rendering (needs a Latin font; skipped when none is available) ───────────

#[cfg(feature = "filters-extra")]
mod render_tests {
    use ferrox_core::font::FontManager;
    use ferrox_core::subtitle::{Subtitle, SubtitleRenderer, SubtitleStyle};
    use ferrox_core::{Frame, PixelFormat};
    use std::fs;

    /// Register a system font that has Latin 'A' under "default"; None if absent.
    fn latin_fonts() -> Option<FontManager> {
        let dirs = ["/System/Library/Fonts", "/usr/share/fonts", "/Library/Fonts"];
        let mut candidates = Vec::new();
        for d in dirs {
            collect_ttf(std::path::Path::new(d), &mut candidates, 0);
        }
        for path in candidates {
            let Ok(bytes) = fs::read(&path) else { continue };
            let fm = FontManager::new();
            if fm.register("default", bytes).is_ok() && fm.has_glyph("default", 'A') {
                return Some(fm);
            }
        }
        None
    }

    fn collect_ttf(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>, depth: u32) {
        if depth > 3 || out.len() > 200 {
            return;
        }
        let Ok(rd) = fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                collect_ttf(&p, out, depth + 1);
            } else if p.extension().and_then(|x| x.to_str()) == Some("ttf") {
                out.push(p);
            }
        }
    }

    fn blank(w: u32, h: u32) -> Frame {
        Frame::new(w, h, PixelFormat::Rgba8, vec![0u8; (w * h * 4) as usize])
    }

    fn drawn_pixels(f: &Frame) -> usize {
        f.data.chunks(4).filter(|p| p[3] != 0).count()
    }

    #[test]
    fn renders_text_onto_frame() {
        let Some(fm) = latin_fonts() else {
            eprintln!("skip: no Latin system font");
            return;
        };
        let subs = Subtitle::from_srt("1\n00:00:00,000 --> 00:00:02,000\nHELLO\n").unwrap();
        let r = SubtitleRenderer::new(&fm);
        let style = SubtitleStyle { size: 40.0, ..Default::default() };

        let mut on = blank(320, 120);
        r.render(&mut on, &subs, 1.0, &style).unwrap();
        assert!(drawn_pixels(&on) > 0, "active cue draws pixels");

        // Outside the cue window nothing is drawn.
        let mut off = blank(320, 120);
        r.render(&mut off, &subs, 5.0, &style).unwrap();
        assert_eq!(drawn_pixels(&off), 0, "inactive cue draws nothing");
    }

    #[test]
    fn background_and_stroke_add_coverage() {
        let Some(fm) = latin_fonts() else {
            eprintln!("skip: no Latin system font");
            return;
        };
        let subs = Subtitle::from_srt("1\n00:00:00,000 --> 00:00:02,000\nHI\n").unwrap();
        let r = SubtitleRenderer::new(&fm);

        let plain = SubtitleStyle { size: 40.0, stroke_width: 0, shadow: None, background: None, ..Default::default() };
        let mut a = blank(240, 100);
        r.render(&mut a, &subs, 1.0, &plain).unwrap();

        let stroked = SubtitleStyle { stroke_width: 3, ..plain.clone() };
        let mut b = blank(240, 100);
        r.render(&mut b, &subs, 1.0, &stroked).unwrap();
        assert!(drawn_pixels(&b) > drawn_pixels(&a), "stroke widens coverage");

        let boxed = SubtitleStyle { background: Some([0, 0, 0, 200]), ..plain };
        let mut c = blank(240, 100);
        r.render(&mut c, &subs, 1.0, &boxed).unwrap();
        assert!(drawn_pixels(&c) > drawn_pixels(&a), "background box fills behind text");
    }

    #[test]
    fn karaoke_highlight_changes_colors_over_time() {
        let Some(fm) = latin_fonts() else {
            eprintln!("skip: no Latin system font");
            return;
        };
        let ass = "[Events]\nFormat: Start, End, Text\n\
Dialogue: 0:00:00.00,0:00:04.00,{\\k100}AAAA{\\k100}BBBB\n";
        let subs = Subtitle::from_ass(ass).unwrap();
        let r = SubtitleRenderer::new(&fm);
        let style = SubtitleStyle {
            size: 40.0,
            color: [255, 255, 255, 255],
            highlight_color: [255, 0, 0, 255],
            stroke_width: 0,
            shadow: None,
            ..Default::default()
        };

        let count_red = |f: &Frame| f.data.chunks(4).filter(|p| p[0] > 120 && p[1] < 80 && p[2] < 80 && p[3] > 0).count();

        let mut early = blank(320, 100);
        r.render(&mut early, &subs, 0.0, &style).unwrap();
        let mut late = blank(320, 100);
        r.render(&mut late, &subs, 3.5, &style).unwrap();
        assert!(count_red(&late) > count_red(&early), "more highlighted glyphs later in the karaoke");
    }
}
