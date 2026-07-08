//! ISO-BMFF box byte-builders shared by the fMP4 and progressive MP4 muxers.

/// Write a 4-byte big-endian u32.
#[inline]
pub(crate) fn be32(v: u32) -> [u8; 4] { v.to_be_bytes() }

/// Write a 8-byte big-endian u64.
#[inline]
pub(crate) fn be64(v: u64) -> [u8; 8] { v.to_be_bytes() }

/// Build a generic MP4 box: 4-byte length + 4-byte type + payload.
pub(crate) fn make_box(fourcc: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let total = 8 + payload.len();
    let mut b = Vec::with_capacity(total);
    b.extend_from_slice(&be32(total as u32));
    b.extend_from_slice(fourcc);
    b.extend_from_slice(payload);
    b
}

/// Make a full box (version + flags prefix).
pub(crate) fn make_full_box(fourcc: &[u8; 4], version: u8, flags: u32, payload: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(4 + payload.len());
    p.push(version);
    p.extend_from_slice(&(flags & 0x00FF_FFFF).to_be_bytes()[1..]);
    p.extend_from_slice(payload);
    make_box(fourcc, &p)
}

// ── ftyp ─────────────────────────────────────────────────────────────────────

pub(crate) fn build_ftyp() -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(b"iso5");  // major brand
    p.extend_from_slice(&be32(512)); // minor version
    p.extend_from_slice(b"iso5");
    p.extend_from_slice(b"iso6");
    p.extend_from_slice(b"mp41");
    make_box(b"ftyp", &p)
}

// ── mvhd ─────────────────────────────────────────────────────────────────────

pub(crate) fn build_mvhd(timescale: u32, duration: u64) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&be64(0u64)); // creation_time
    p.extend_from_slice(&be64(0u64)); // modification_time
    p.extend_from_slice(&be32(timescale));
    p.extend_from_slice(&be64(duration));
    p.extend_from_slice(&be32(0x0001_0000)); // rate = 1.0
    p.extend_from_slice(&[0x01, 0x00]); // volume = 1.0
    p.extend_from_slice(&[0u8; 10]); // reserved
    // unity matrix
    p.extend_from_slice(&be32(0x0001_0000)); p.extend_from_slice(&be32(0));
    p.extend_from_slice(&be32(0));            p.extend_from_slice(&be32(0));
    p.extend_from_slice(&be32(0x0001_0000)); p.extend_from_slice(&be32(0));
    p.extend_from_slice(&be32(0));            p.extend_from_slice(&be32(0));
    p.extend_from_slice(&be32(0x4000_0000));
    p.extend_from_slice(&[0u8; 24]); // pre-defined
    p.extend_from_slice(&be32(0xFFFF_FFFE)); // next_track_id placeholder
    make_full_box(b"mvhd", 1, 0, &p)
}

// ── tkhd ─────────────────────────────────────────────────────────────────────

pub(crate) fn build_tkhd(track_id: u32, duration: u64, width: u32, height: u32, is_audio: bool) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&be64(0u64)); // creation_time
    p.extend_from_slice(&be64(0u64)); // modification_time
    p.extend_from_slice(&be32(track_id));
    p.extend_from_slice(&be32(0)); // reserved
    p.extend_from_slice(&be64(duration));
    p.extend_from_slice(&[0u8; 8]); // reserved
    p.extend_from_slice(&[0u8; 2]); // layer
    p.extend_from_slice(&[0u8; 2]); // alternate_group
    let vol: [u8; 2] = if is_audio { [0x01, 0x00] } else { [0u8; 2] };
    p.extend_from_slice(&vol);
    p.extend_from_slice(&[0u8; 2]); // reserved
    // unity matrix
    p.extend_from_slice(&be32(0x0001_0000)); p.extend_from_slice(&be32(0));
    p.extend_from_slice(&be32(0));            p.extend_from_slice(&be32(0));
    p.extend_from_slice(&be32(0x0001_0000)); p.extend_from_slice(&be32(0));
    p.extend_from_slice(&be32(0));            p.extend_from_slice(&be32(0));
    p.extend_from_slice(&be32(0x4000_0000));
    // width/height as 16.16 fixed point (0 for audio)
    if is_audio {
        p.extend_from_slice(&[0u8; 8]);
    } else {
        p.extend_from_slice(&be32(width << 16));
        p.extend_from_slice(&be32(height << 16));
    }
    // flags=3: track_enabled | track_in_movie
    make_full_box(b"tkhd", 1, 3, &p)
}

// ── mdhd ─────────────────────────────────────────────────────────────────────

pub(crate) fn build_mdhd(timescale: u32, duration: u64) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&be64(0u64)); // creation_time
    p.extend_from_slice(&be64(0u64)); // modification_time
    p.extend_from_slice(&be32(timescale));
    p.extend_from_slice(&be64(duration));
    p.extend_from_slice(&[0x55, 0xC4]); // language = 'und' (ISO 639-2/T)
    p.extend_from_slice(&be32(0)); // pre_defined
    make_full_box(b"mdhd", 1, 0, &p)
}

// ── hdlr ─────────────────────────────────────────────────────────────────────

pub(crate) fn build_hdlr(handler: &[u8; 4], name: &[u8]) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&be32(0)); // pre_defined
    p.extend_from_slice(handler);
    p.extend_from_slice(&[0u8; 12]); // reserved
    p.extend_from_slice(name);
    p.push(0); // null terminator
    make_full_box(b"hdlr", 0, 0, &p)
}

// ── sample entries ────────────────────────────────────────────────────────────

pub(crate) fn build_avc1(width: u32, height: u32, avcc: &[u8]) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&[0u8; 6]); // reserved
    p.extend_from_slice(&be32(1)[2..]); // data_reference_index = 1 (u16)
    p.extend_from_slice(&[0u8; 16]); // pre_defined + reserved
    p.extend_from_slice(&be32(width)[2..]);  // width (u16)
    p.extend_from_slice(&be32(height)[2..]); // height (u16)
    p.extend_from_slice(&be32(0x0048_0000)); // horizresolution = 72dpi
    p.extend_from_slice(&be32(0x0048_0000)); // vertresolution
    p.extend_from_slice(&be32(0)); // reserved
    p.extend_from_slice(&be32(1)[2..]); // frame_count = 1 (u16)
    p.extend_from_slice(&[0u8; 32]); // compressorname
    p.extend_from_slice(&[0x00, 0x18]); // depth = 24
    p.extend_from_slice(&[0xFF, 0xFF]); // pre_defined = -1

    // avcC box
    let avcc_box = make_box(b"avcC", avcc);
    p.extend_from_slice(&avcc_box);
    make_box(b"avc1", &p)
}

pub(crate) fn build_av01(width: u32, height: u32) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&[0u8; 6]); // reserved
    p.extend_from_slice(&[0x00, 0x01]); // data_reference_index
    p.extend_from_slice(&[0u8; 16]);
    p.extend_from_slice(&be32(width)[2..]);
    p.extend_from_slice(&be32(height)[2..]);
    p.extend_from_slice(&be32(0x0048_0000));
    p.extend_from_slice(&be32(0x0048_0000));
    p.extend_from_slice(&be32(0));
    p.extend_from_slice(&[0x00, 0x01]);
    p.extend_from_slice(&[0u8; 32]);
    p.extend_from_slice(&[0x00, 0x18]);
    p.extend_from_slice(&[0xFF, 0xFF]);
    // Minimal av1C (sequence header placeholder — real encoders write this)
    let av1c = make_box(b"av1C", &[0x81, 0x04, 0x0C, 0x00]);
    p.extend_from_slice(&av1c);
    make_box(b"av01", &p)
}

pub(crate) fn build_mp4a(sample_rate: u32, channels: u32) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&[0u8; 6]);
    p.extend_from_slice(&[0x00, 0x01]);
    p.extend_from_slice(&[0u8; 8]);
    p.extend_from_slice(&be32(channels)[2..]); // channelcount (u16)
    p.extend_from_slice(&[0x00, 0x10]); // samplesize = 16
    p.extend_from_slice(&[0u8; 4]);
    p.extend_from_slice(&be32(sample_rate << 16)); // samplerate 16.16
    // esds (minimal, describing AAC LC)
    let esds = build_esds(channels as u8, sample_rate);
    p.extend_from_slice(&esds);
    make_box(b"mp4a", &p)
}

pub(crate) fn build_esds(channels: u8, sample_rate: u32) -> Vec<u8> {
    // Encode sample_rate index for AAC
    let sri: u8 = match sample_rate {
        96000 => 0, 88200 => 1, 64000 => 2, 48000 => 3,
        44100 => 4, 32000 => 5, 24000 => 6, 22050 => 7,
        16000 => 8, 12000 => 9, 11025 => 10, 8000 => 11,
        _ => 4,
    };
    // AudioSpecificConfig: AAC-LC, sample_rate_index, channels
    let asc: [u8; 2] = [0x11 | ((sri >> 1) << 3), ((sri & 1) << 7) | (channels << 3)];
    // DecoderSpecificInfo tag (0x05)
    let mut dsi = vec![0x05u8, asc.len() as u8];
    dsi.extend_from_slice(&asc);
    // DecoderConfigDescriptor tag (0x04): objectTypeIndication=0x40 (AAC), streamType=0x15, bufferSize=0, maxBitrate/avgBitrate
    let mut dcd = vec![
        0x04u8, (13 + dsi.len()) as u8,
        0x40, // objectTypeIndication = Audio ISO/IEC 14496-3
        0x15, // streamType=0x05 (AudioStream) <<1 | upStream=0 | 1
        0x00, 0x00, 0x00, // bufferSizeDB
        0x00, 0x00, 0x00, 0x00, // maxBitrate
        0x00, 0x00, 0x00, 0x00, // avgBitrate
    ];
    dcd.extend_from_slice(&dsi);
    // SLConfigDescriptor (0x06): predefined=2
    let slcd = vec![0x06u8, 0x01, 0x02];
    // ES_Descriptor (0x03)
    let mut esd = vec![0x03u8, (3 + dcd.len() + slcd.len()) as u8, 0x00, 0x00, 0x00];
    esd.extend_from_slice(&dcd);
    esd.extend_from_slice(&slcd);
    make_full_box(b"esds", 0, 0, &esd)
}

// ── stsd / stts / stsc / stsz / stco (empty, for fragmented MP4) ─────────────

