pub mod audio;
pub mod codecs;
#[cfg(feature = "wasm")]
pub mod wasm;
pub mod error;
pub mod filter_graph;
pub mod filters;
pub mod formats;
pub mod frame;
pub mod graph;
pub mod media;
pub mod simd_ops;
#[cfg(feature = "gpu")]
pub mod gpu;
#[cfg(feature = "gpu")]
pub use gpu::{BlurGpu, GpuFilter, ResizeGpu};
pub mod anim;
pub mod blend;
pub mod color;
pub mod keyer;
pub mod mask;
pub mod registry;
pub mod timeline;
pub mod transitions;
pub mod compositor;
pub mod traits;
pub mod video;
#[cfg(feature = "video-codecs")]
pub mod demux_graph;

pub use audio::{AudioFormat, AudioFrame};
pub use error::{Error, Result};
pub use filter_graph::{FilterGraph, FilterPlugin};
pub use filters::{
    BlurFilter, BrightnessFilter, ContrastFilter, CropFilter,
    FlipAxis, FlipFilter, GrayscaleFilter, NegateFilter,
    OverlayFilter, PadFilter, ResampleFilter, ResizeAlgorithm,
    ResizeFilter, RotateFilter, Rotation, SaturationFilter,
    ThumbnailFilter, VolumeFilter,
};
#[cfg(feature = "filters-extra")]
pub use filters::{TextColor, DrawTextFilter};
pub use frame::{Frame, PixelFormat};
pub use anim::{Curve, Easing, Keyframe};
pub use blend::BlendMode;
pub use color::{AscCdl, ColorGrade, Lut3D};
pub use keyer::Keyer;
pub use mask::Mask;
pub use transitions::{Direction, Transition};
pub use timeline::{
    AudioClip, AudioClipSource, AudioTrack, Clip, ClipAnimation, ClipSource, Fade, Project, Track,
    Transform,
};
pub use compositor::{compose_frame, compose_frame_graded};
pub use audio::effects::{apply_effects, AudioEffect, EqBand, EqKind};
pub use audio::mixer::{mix, mix_full, render_audio};
pub use audio::waveform::{generate_waveform, WaveformBucket};
pub use graph::{AudioGraph, Graph};
pub use media::{MediaFrame, MediaType};
pub use codecs::{Mp3Decoder, SymphoniaDecoder};
#[cfg(feature = "mp3-encode")]
pub use codecs::{Mp3Encoder, Mp3Options, Mp3Quality};
#[cfg(feature = "opus-encode")]
pub use codecs::{OpusEncoder, OpusOptions, OpusApplication};
pub use traits::{
    AudioDecoder, AudioEncoder, AudioFilter,
    Decoder, DynDecoder, DynEncoder, Encoder, Filter, Format, MediaFilter,
};
pub use video::{CodecId, Packet, StreamInfo, StreamKind, VideoFrame};
#[cfg(feature = "video-codecs")]
pub use codecs::video::{IvfDemuxer, Mp4Demuxer, Vp8Decoder, WebmDemuxer};
#[cfg(feature = "vp9")]
pub use codecs::video::Vp9Decoder;
#[cfg(feature = "h264")]
pub use codecs::video::H264Decoder;
#[cfg(feature = "h264")]
pub use codecs::video::h264::{H264OutputMode, H264Profile, detect_h264_profile};
#[cfg(feature = "video-codecs")]
pub use traits::{ContainerDemuxer, VideoDecoder};
#[cfg(feature = "video-codecs")]
pub use demux_graph::{
    extract_audio, extract_frames, ContainerKind, ExtractResult,
    any_yuv_to_rgb8, yuv420p_to_rgb8, yuv420p_hdr_to_rgb8, yuv422p_to_rgb8, yuv444p_to_rgb8,
};
#[cfg(feature = "encode")]
pub use codecs::video::{Av1Encoder, FMp4Muxer, MpegTsMuxer, WebmMuxer, build_fmp4_init, build_fmp4_segment};
#[cfg(feature = "encode")]
pub use traits::{ContainerMuxer, VideoEncoder};
#[cfg(feature = "encode")]
pub use video::EncodedPacket;
#[cfg(feature = "encode")]
pub mod transcode_graph;
#[cfg(feature = "encode")]
pub mod hls;
#[cfg(feature = "encode")]
pub use hls::{segment as hls_segment, HlsOptions, HlsResult, HlsSegmentFormat, SegmentInfo, parse_m3u8, M3u8Playlist, M3u8Segment};
#[cfg(feature = "gif-support")]
pub use codecs::gif::{decode_gif, encode_gif, GifEncodeOptions, GifFrame};
