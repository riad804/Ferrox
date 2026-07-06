//! HLS segmentation internals: demux → decode/encode → segment files + M3U8.

use std::{
    fmt::Write as FmtWrite,
    fs::{self, File},
    io::BufWriter,
    path::{Path, PathBuf},
};

use crate::{
    codecs::video::{
        Av1Encoder, MpegTsMuxer, WebmMuxer,
        build_fmp4_init, build_fmp4_segment,
    },
    error::{Error, Result},
    traits::{ContainerDemuxer, ContainerMuxer, VideoDecoder, VideoEncoder},
    video::{CodecId, EncodedPacket, StreamInfo, StreamKind},
};
use super::{HlsOptions, HlsResult, HlsSegmentFormat, SegmentInfo};

pub(super) fn segment_with_demuxer<D: ContainerDemuxer>(
    mut demuxer: D,
    opts: &HlsOptions,
) -> Result<HlsResult> {
    let streams = demuxer.streams().to_vec();
    let video_stream = streams.iter().find(|s| s.is_video())
        .ok_or_else(|| Error::Video("no video stream found".into()))?;

    let src_w   = video_stream.width;
    let src_h   = video_stream.height;
    let fps     = video_stream.frame_rate;
    let codec   = video_stream.codec.clone();
    let vid_idx = video_stream.index;

    if codec != CodecId::Vp8 {
        return Err(Error::Video(format!(
            "HLS segmenter: source codec {codec} is not decodable; only VP8 is supported in the default build"
        )));
    }

    let (fps_num, fps_den) = if fps > 0.0 {
        ((fps * 1000.0).round() as u64, 1000u64)
    } else {
        (30_000, 1000)
    };

    let frames_per_seg =
        ((opts.segment_duration_secs * fps_num as f64 / fps_den as f64).ceil() as usize).max(1);

    let out_stream = StreamInfo {
        index: 0,
        kind: StreamKind::Video,
        codec: CodecId::Av1,
        width: src_w,
        height: src_h,
        frame_rate: fps_num as f64 / fps_den as f64,
        sample_rate: 0,
        channels: 0,
        codec_private: Vec::new(),
    };

    // For fMP4: write the init segment once.
    let init_path: Option<PathBuf> = if opts.format == HlsSegmentFormat::FMp4 {
        let init_bytes = build_fmp4_init(&[out_stream.clone()]);
        let p = opts.output_dir.join(format!("{}init.mp4", opts.segment_prefix));
        fs::write(&p, &init_bytes).map_err(Error::Io)?;
        Some(p)
    } else {
        None
    };

    let mut decoder = crate::codecs::video::Vp8Decoder;
    let mut segments: Vec<SegmentInfo> = Vec::new();
    let mut seg_idx  = 0usize;
    let mut total_frames = 0usize;

    let mut seg_pkts:  Vec<EncodedPacket> = Vec::new();
    let mut seg_frames = 0usize;

    // fMP4 segment state (sequence number + running base_decode_time in 90 kHz)
    let mut fmp4_seq: u32 = 1;
    let mut fmp4_bdt: u64 = 0;

    let mut encoder = Av1Encoder::new(src_w, src_h, opts.speed, opts.quantizer, fps_num, fps_den)?;

    loop {
        let Some((stream_idx, packet)) = demuxer.next_packet()? else { break };
        if stream_idx != vid_idx { continue; }

        let vf = match decoder.decode_packet(&packet) {
            Ok(f) => f,
            Err(_) => continue,
        };
        total_frames += 1;

        let is_boundary = vf.is_keyframe && seg_frames >= frames_per_seg;
        if is_boundary && !seg_pkts.is_empty() {
            let (bdt_after, path) = flush_segment(
                &seg_pkts, seg_idx, seg_frames,
                &out_stream, opts, fps_num, fps_den,
                fmp4_seq, fmp4_bdt,
                &mut segments,
            )?;
            fmp4_seq += 1;
            fmp4_bdt  = bdt_after;
            let _ = path;

            seg_pkts.clear();
            seg_frames = 0;
            seg_idx += 1;
            encoder = Av1Encoder::new(src_w, src_h, opts.speed, opts.quantizer, fps_num, fps_den)?;
        }

        let pkts = encoder.encode(&vf)?;
        seg_pkts.extend(pkts);
        seg_frames += 1;
    }

    // Flush last segment.
    let flushed = encoder.flush()?;
    seg_pkts.extend(flushed);
    if !seg_pkts.is_empty() {
        flush_segment(
            &seg_pkts, seg_idx, seg_frames,
            &out_stream, opts, fps_num, fps_den,
            fmp4_seq, fmp4_bdt,
            &mut segments,
        )?;
    }

    // Write M3U8.
    let playlist_path = opts.output_dir.join(&opts.playlist_name);
    write_m3u8(&playlist_path, &segments, opts, init_path.as_deref())?;

    Ok(HlsResult { playlist_path, segments, total_frames })
}

/// Flush one segment; returns `(new_fmp4_base_decode_time, path)`.
fn flush_segment(
    pkts: &[EncodedPacket],
    seg_idx: usize,
    nframes: usize,
    out_stream: &StreamInfo,
    opts: &HlsOptions,
    fps_num: u64,
    fps_den: u64,
    fmp4_seq: u32,
    fmp4_bdt: u64,
    segments: &mut Vec<SegmentInfo>,
) -> Result<(u64, PathBuf)> {
    if pkts.is_empty() {
        let p = opts.output_dir.join("empty");
        return Ok((fmp4_bdt, p));
    }

    let ext = opts.format.extension();
    let filename = format!("{}{:03}.{ext}", opts.segment_prefix, seg_idx);
    let path = opts.output_dir.join(&filename);

    let fps_f = fps_num as f64 / fps_den as f64;
    let duration_secs = if fps_f > 0.0 { nframes as f64 / fps_f } else { 0.0 };

    let new_bdt = match &opts.format {
        HlsSegmentFormat::WebM => {
            let f = BufWriter::new(File::create(&path)?);
            let mut muxer = WebmMuxer::new(f, &[out_stream.clone()], fps_num, fps_den)?;
            muxer.write_header()?;
            for pkt in pkts { muxer.write_packet(pkt)?; }
            muxer.write_trailer()?;
            fmp4_bdt
        }
        HlsSegmentFormat::FMp4 => {
            // Each HLS segment is a self-contained moof+mdat (no ftyp/moov).
            let timescale = 90_000u32;
            let seg_bytes = build_fmp4_segment(
                fmp4_seq, 1, timescale, fmp4_bdt, pkts, fps_num, fps_den,
            );
            fs::write(&path, &seg_bytes).map_err(Error::Io)?;

            // Advance base_decode_time by the total frame duration (in timescale units).
            let total_dur: u64 = pkts.iter().map(|p| p.duration.max(1)).sum();
            let scaled = if fps_num > 0 {
                total_dur * fps_den * timescale as u64 / fps_num
            } else {
                total_dur
            };
            fmp4_bdt + scaled
        }
        HlsSegmentFormat::MpegTs => {
            let mut buf: Vec<u8> = Vec::new();
            let mut muxer = MpegTsMuxer::new(&mut buf, &[out_stream.clone()], fps_num, fps_den)?;
            muxer.write_header()?;
            for pkt in pkts { muxer.write_packet(pkt)?; }
            muxer.write_trailer()?;
            fs::write(&path, &buf).map_err(Error::Io)?;
            fmp4_bdt
        }
    };

    segments.push(SegmentInfo { path: path.clone(), duration_secs, frames: nframes });
    Ok((new_bdt, path))
}

// ── M3U8 writer ───────────────────────────────────────────────────────────────

fn write_m3u8(
    path: &Path,
    segments: &[SegmentInfo],
    opts: &HlsOptions,
    init_path: Option<&Path>,
) -> Result<()> {
    let max_dur = segments.iter().map(|s| s.duration_secs).fold(0.0_f64, f64::max);
    let version = opts.format.hls_version();

    let mut m3u8 = String::new();
    writeln!(m3u8, "#EXTM3U").unwrap();
    writeln!(m3u8, "#EXT-X-VERSION:{version}").unwrap();
    writeln!(m3u8, "#EXT-X-TARGETDURATION:{}", max_dur.ceil() as u64).unwrap();
    writeln!(m3u8, "#EXT-X-MEDIA-SEQUENCE:0").unwrap();

    // fMP4 requires an EXT-X-MAP pointing to the init segment.
    if let Some(ip) = init_path {
        let name = ip.file_name().and_then(|n| n.to_str()).unwrap_or("init.mp4");
        writeln!(m3u8, "#EXT-X-MAP:URI=\"{name}\"").unwrap();
    }

    for seg in segments {
        let filename = seg.path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("segment");
        writeln!(m3u8, "#EXTINF:{:.6},", seg.duration_secs).unwrap();
        writeln!(m3u8, "{filename}").unwrap();
    }
    writeln!(m3u8, "#EXT-X-ENDLIST").unwrap();

    fs::write(path, m3u8).map_err(Error::Io)?;
    Ok(())
}

