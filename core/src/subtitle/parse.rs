//! Subtitle parsers: SRT, WebVTT, and ASS/SSA (including `\k` karaoke timing).

use crate::error::{Error, Result};

use super::model::{Cue, KaraokeSegment, Subtitle};

/// A subtitle container format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitleFormat {
    Srt,
    Vtt,
    Ass,
}

impl Subtitle {
    /// Parse subtitle text in the given format.
    pub fn parse(text: &str, format: SubtitleFormat) -> Result<Self> {
        match format {
            SubtitleFormat::Srt => parse_srt(text),
            SubtitleFormat::Vtt => parse_vtt(text),
            SubtitleFormat::Ass => parse_ass(text),
        }
    }

    pub fn from_srt(text: &str) -> Result<Self> {
        parse_srt(text)
    }
    pub fn from_vtt(text: &str) -> Result<Self> {
        parse_vtt(text)
    }
    pub fn from_ass(text: &str) -> Result<Self> {
        parse_ass(text)
    }
}

// ── time parsing ────────────────────────────────────────────────────────────

/// `HH:MM:SS,mmm` (SRT) or `HH:MM:SS.mmm` / `MM:SS.mmm` (VTT).
fn parse_clock(s: &str) -> Option<f64> {
    let s = s.trim().replace(',', ".");
    let (hms, frac) = s.split_once('.').unwrap_or((&s, "0"));
    let parts: Vec<&str> = hms.split(':').collect();
    let (h, m, sec) = match parts.as_slice() {
        [h, m, s] => (h.parse::<f64>().ok()?, m.parse::<f64>().ok()?, s.parse::<f64>().ok()?),
        [m, s] => (0.0, m.parse::<f64>().ok()?, s.parse::<f64>().ok()?),
        _ => return None,
    };
    let millis = format!("0.{frac}").parse::<f64>().unwrap_or(0.0);
    Some(h * 3600.0 + m * 60.0 + sec + millis)
}

/// ASS time `H:MM:SS.cc` (centiseconds).
fn parse_ass_time(s: &str) -> Option<f64> {
    parse_clock(s)
}

fn parse_arrow(line: &str) -> Option<(f64, f64)> {
    let (a, b) = line.split_once("-->")?;
    // VTT may append cue settings after the end time.
    let b = b.split_whitespace().next().unwrap_or(b);
    Some((parse_clock(a)?, parse_clock(b)?))
}

// ── SRT ─────────────────────────────────────────────────────────────────────

fn parse_srt(text: &str) -> Result<Subtitle> {
    let mut cues = Vec::new();
    for block in text.split("\n\n").map(|b| b.replace('\r', "")) {
        let lines: Vec<&str> = block.lines().filter(|l| !l.trim().is_empty()).collect();
        if lines.is_empty() {
            continue;
        }
        // Optional numeric index line; the timing line contains "-->".
        let ts_idx = lines.iter().position(|l| l.contains("-->"));
        let Some(ti) = ts_idx else { continue };
        let Some((start, end)) = parse_arrow(lines[ti]) else {
            return Err(Error::Filter(format!("bad SRT timing: {}", lines[ti])));
        };
        let text = lines[ti + 1..].join("\n");
        if !text.is_empty() {
            cues.push(Cue::new(start, end, text));
        }
    }
    Ok(Subtitle::new(cues))
}

// ── WebVTT ──────────────────────────────────────────────────────────────────

fn parse_vtt(text: &str) -> Result<Subtitle> {
    let mut cues = Vec::new();
    for block in text.split("\n\n").map(|b| b.replace('\r', "")) {
        let lines: Vec<&str> = block.lines().collect();
        let Some(ti) = lines.iter().position(|l| l.contains("-->")) else { continue };
        if lines[..ti].iter().any(|l| l.trim() == "WEBVTT") && ti == 0 {
            continue;
        }
        let Some((start, end)) = parse_arrow(lines[ti]) else { continue };
        let text = lines[ti + 1..].join("\n");
        let text = strip_vtt_tags(&text);
        if !text.trim().is_empty() {
            cues.push(Cue::new(start, end, text));
        }
    }
    Ok(Subtitle::new(cues))
}

/// Remove VTT inline tags like `<c.classname>` / `<b>`.
fn strip_vtt_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

// ── ASS / SSA ───────────────────────────────────────────────────────────────

fn parse_ass(text: &str) -> Result<Subtitle> {
    let mut cues = Vec::new();
    let mut in_events = false;
    let mut format: Vec<String> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.eq_ignore_ascii_case("[Events]") {
            in_events = true;
            continue;
        }
        if line.starts_with('[') {
            in_events = false;
            continue;
        }
        if !in_events {
            continue;
        }
        if let Some(rest) = line.strip_prefix("Format:") {
            format = rest.split(',').map(|s| s.trim().to_ascii_lowercase()).collect();
        } else if let Some(rest) = line.strip_prefix("Dialogue:") {
            if let Some(cue) = parse_ass_dialogue(rest, &format) {
                cues.push(cue);
            }
        }
    }
    Ok(Subtitle::new(cues))
}

fn parse_ass_dialogue(rest: &str, format: &[String]) -> Option<Cue> {
    // The Text field is last and may itself contain commas, so split at most
    // `format.len()` fields.
    let n = format.len().max(1);
    let fields: Vec<&str> = rest.splitn(n, ',').collect();
    let idx = |name: &str| format.iter().position(|f| f == name);
    let start = parse_ass_time(fields.get(idx("start")?)?)?;
    let end = parse_ass_time(fields.get(idx("end")?)?)?;
    let raw_text = fields.get(idx("text")?)?.to_string();

    let (plain, segments) = parse_karaoke(&raw_text, start);
    Some(Cue { start, end, text: plain, segments })
}

/// Split ASS text into plain text + karaoke segments from `{\kNN}` tags.
/// Returns `(plain_text, segments)`; `segments` is empty when there is no `\k`.
fn parse_karaoke(text: &str, cue_start: f64) -> (String, Vec<KaraokeSegment>) {
    let mut plain = String::new();
    let mut segments = Vec::new();
    let mut cursor = 0.0f64; // seconds after cue start
    let mut pending: Option<f64> = None; // duration of the current \k, seconds
    let mut chars = text.char_indices().peekable();

    while let Some((_, ch)) = chars.next() {
        if ch == '{' {
            // Read an override block up to '}'.
            let mut block = String::new();
            for (_, c) in chars.by_ref() {
                if c == '}' {
                    break;
                }
                block.push(c);
            }
            if let Some(cs) = parse_k_tag(&block) {
                pending = Some(cs);
            }
        } else {
            // Accumulate a syllable until the next '{' (or end).
            let mut syl = String::new();
            syl.push(ch);
            while let Some((_, c)) = chars.peek() {
                if *c == '{' {
                    break;
                }
                syl.push(*c);
                chars.next();
            }
            plain.push_str(&syl);
            if let Some(dur) = pending.take() {
                segments.push(KaraokeSegment { text: syl, start: cursor, duration: dur });
                cursor += dur;
            }
        }
    }
    let _ = cue_start;
    (plain, segments)
}

/// Parse a `\kNN` / `\KfNN` centisecond value from an override block.
fn parse_k_tag(block: &str) -> Option<f64> {
    let mut chars = block.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c == '\\' {
            chars.next();
            let tag: String = std::iter::from_fn(|| chars.next_if(|c| c.is_ascii_alphabetic())).collect();
            if tag.eq_ignore_ascii_case("k") || tag.eq_ignore_ascii_case("kf") || tag.eq_ignore_ascii_case("ko") {
                let num: String = std::iter::from_fn(|| chars.next_if(|c| c.is_ascii_digit())).collect();
                if let Ok(cs) = num.parse::<f64>() {
                    return Some(cs / 100.0); // centiseconds → seconds
                }
            }
        } else {
            chars.next();
        }
    }
    None
}
