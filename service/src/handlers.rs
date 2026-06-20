//! Axum handler functions for each route.

use axum::{
    body::Body,
    extract::Multipart,
    http::{header, StatusCode},
    response::{IntoResponse, Json, Response},
};
use serde_json::json;
use tracing::{info, warn};

use ferrox_core::{
    codecs::{JpegDecoder, JpegEncoder, PngDecoder, PngEncoder},
    filter_graph::FilterGraph,
    frame::{Frame, PixelFormat},
    traits::{DynDecoder, DynEncoder},
};

use crate::models::{ApiError, Job};

// ── /health ───────────────────────────────────────────────────────────────────

/// Liveness probe.
pub async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}

// ── /probe ────────────────────────────────────────────────────────────────────

/// Accept a multipart `file` field, decode it, return stream metadata as JSON.
pub async fn probe(mut multipart: Multipart) -> Response {
    let bytes = match read_file_field(&mut multipart).await {
        Ok(b) => b,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, e),
    };

    match probe_bytes(&bytes) {
        Ok(info) => Json(info).into_response(),
        Err(e) => error_response(StatusCode::UNPROCESSABLE_ENTITY, e),
    }
}

fn probe_bytes(bytes: &[u8]) -> Result<serde_json::Value, String> {
    // Sniff format from magic bytes.
    let (frame, format_name) = decode_image_bytes(bytes)?;
    Ok(json!({
        "format": format_name,
        "streams": [{
            "index": 0,
            "kind": "video",
            "codec": format_name.to_uppercase(),
            "width":  frame.width,
            "height": frame.height,
            "pixel_format": format!("{:?}", frame.format),
        }]
    }))
}

// ── /process ──────────────────────────────────────────────────────────────────

/// Accept multipart `file` + `job` fields, apply the job, stream back result.
pub async fn process(mut multipart: Multipart) -> Response {
    let mut file_bytes: Option<bytes::Bytes> = None;
    let mut job_str: Option<String> = None;

    // Collect all fields.
    loop {
        match multipart.next_field().await {
            Ok(Some(field)) => {
                let name = field.name().unwrap_or("").to_string();
                match name.as_str() {
                    "file" => {
                        match field.bytes().await {
                            Ok(b) => file_bytes = Some(b),
                            Err(e) => return error_response(StatusCode::BAD_REQUEST, format!("reading file: {e}")),
                        }
                    }
                    "job" => {
                        match field.text().await {
                            Ok(t) => job_str = Some(t),
                            Err(e) => return error_response(StatusCode::BAD_REQUEST, format!("reading job: {e}")),
                        }
                    }
                    other => {
                        warn!("unknown multipart field '{other}', skipping");
                        // consume and discard
                        let _ = field.bytes().await;
                    }
                }
            }
            Ok(None) => break,
            Err(e) => return error_response(StatusCode::BAD_REQUEST, format!("multipart error: {e}")),
        }
    }

    let bytes = match file_bytes {
        Some(b) => b,
        None => return error_response(StatusCode::BAD_REQUEST, "missing 'file' field"),
    };
    let job: Job = match job_str {
        Some(s) => match serde_json::from_str(&s) {
            Ok(j) => j,
            Err(e) => return error_response(StatusCode::BAD_REQUEST, format!("invalid job JSON: {e}")),
        },
        None => return error_response(StatusCode::BAD_REQUEST, "missing 'job' field"),
    };

    info!(
        output_format = job.output_format,
        filter_complex = ?job.filter_complex,
        bytes = bytes.len(),
        "processing job"
    );

    // Decode input.
    let (frame, _fmt) = match decode_image_bytes(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(StatusCode::UNPROCESSABLE_ENTITY, e),
    };

    // Apply filters.
    let out_frame = match &job.filter_complex {
        Some(expr) => match FilterGraph::parse_and_run(frame, expr) {
            Ok(f) => f,
            Err(e) => return error_response(StatusCode::UNPROCESSABLE_ENTITY, format!("filter error: {e}")),
        },
        None => frame,
    };

    // Encode output.
    let out_fmt = job.output_format.to_ascii_lowercase();
    let (out_bytes, mime, filename) = match encode_frame(&out_frame, &out_fmt) {
        Ok(r) => r,
        Err(e) => return error_response(StatusCode::UNPROCESSABLE_ENTITY, e),
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        )
        .body(Body::from(out_bytes))
        .unwrap()
}

// ── helpers ───────────────────────────────────────────────────────────────────

async fn read_file_field(multipart: &mut Multipart) -> Result<bytes::Bytes, String> {
    loop {
        match multipart.next_field().await {
            Ok(Some(field)) => {
                if field.name() == Some("file") {
                    return field.bytes().await.map_err(|e| format!("reading file bytes: {e}"));
                }
                // skip non-file fields
                let _ = field.bytes().await;
            }
            Ok(None) => return Err("missing 'file' field in multipart body".into()),
            Err(e) => return Err(format!("multipart error: {e}")),
        }
    }
}

/// Sniff format from magic bytes and decode to a `Frame`.
fn decode_image_bytes(bytes: &[u8]) -> Result<(Frame, &'static str), String> {
    use std::io::Cursor;

    // PNG magic: \x89PNG
    if bytes.starts_with(b"\x89PNG") {
        let frame = PngDecoder
            .decode_dyn(&mut Cursor::new(bytes))
            .map_err(|e| format!("PNG decode error: {e}"))?;
        return Ok((frame, "png"));
    }
    // JPEG magic: FF D8
    if bytes.starts_with(b"\xff\xd8") {
        let frame = JpegDecoder
            .decode_dyn(&mut Cursor::new(bytes))
            .map_err(|e| format!("JPEG decode error: {e}"))?;
        return Ok((frame, "jpeg"));
    }
    Err("unsupported image format (only PNG and JPEG are supported)".into())
}

/// Encode a `Frame` to the requested output format.
/// Returns `(bytes, mime_type, filename)`.
fn encode_frame(
    frame: &Frame,
    out_fmt: &str,
) -> Result<(Vec<u8>, &'static str, String), String> {
    // Ensure frame is Rgb8 for encoding (strip alpha if needed).
    let rgb_frame = match frame.format {
        PixelFormat::Rgba8 => {
            let rgb: Vec<u8> = frame.data.chunks_exact(4)
                .flat_map(|p| [p[0], p[1], p[2]])
                .collect();
            Frame::new(frame.width, frame.height, PixelFormat::Rgb8, rgb)
        }
        _ => frame.clone(),
    };

    let mut buf = Vec::new();
    match out_fmt {
        "png" => {
            PngEncoder
                .encode_dyn(&rgb_frame, &mut buf)
                .map_err(|e| format!("PNG encode error: {e}"))?;
            Ok((buf, "image/png", "output.png".into()))
        }
        "jpg" | "jpeg" => {
            JpegEncoder::default()
                .encode_dyn(&rgb_frame, &mut buf)
                .map_err(|e| format!("JPEG encode error: {e}"))?;
            Ok((buf, "image/jpeg", "output.jpg".into()))
        }
        other => Err(format!("unsupported output format '{other}'; use png or jpg")),
    }
}

fn error_response(status: StatusCode, msg: impl Into<String>) -> Response {
    let body = Json(ApiError::new(msg));
    (status, body).into_response()
}
