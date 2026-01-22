use crate::SharedPlaybackState;
use anyhow::Result;
use anyhow::bail;
use egui::ColorImage;
use std::fmt::{Display, Formatter};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU8, AtomicU32};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread::JoinHandle;

#[cfg(feature = "avfoundation")]
mod avfoundation;
#[cfg(feature = "ffmpeg")]
mod ffmpeg;

#[derive(Clone, Debug)]
pub struct DecoderInfo {
    pub bitrate: u64,
    pub duration: f32,
    pub streams: Vec<StreamInfo>,
}

#[derive(Clone, Debug)]
pub enum StreamType {
    Video,
    Audio,
    Subtitle,
}

#[derive(Clone, Debug)]
pub struct StreamInfo {
    pub r#type: StreamType,
    pub index: i32,
    pub codec: String,
    pub format: String,
    pub channels: u8,
    pub sample_rate: u32,
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub language: Option<String>,
}

impl Display for StreamInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self.r#type {
            StreamType::Video => {
                write!(
                    f,
                    "V #{}: {} {}x{}@{} {}fps",
                    self.index, self.codec, self.width, self.height, self.format, self.fps
                )
            }
            StreamType::Audio => {
                write!(
                    f,
                    "A #{}: {} {}ch {}@{} {}",
                    self.index,
                    self.codec,
                    self.channels,
                    self.format,
                    self.sample_rate,
                    self.language.as_ref().map(|s| s.as_str()).unwrap_or("unk")
                )
            }
            StreamType::Subtitle => {
                write!(
                    f,
                    "S #{}: {} {}",
                    self.index,
                    self.codec,
                    self.language.as_ref().map(|s| s.as_str()).unwrap_or("unk")
                )
            }
        }
    }
}

#[derive(Clone)]
pub struct VideoFrame {
    /// Frame as an egui image
    pub data: ColorImage,
    /// The stream index this frame belongs to
    pub stream_index: i32,
    /// Presentation timestamp
    pub pts: f64,
    /// Duration this frame should be shown
    pub duration: f64,
}

#[derive(Clone)]
pub struct AudioSamples {
    /// Raw audio samples, must be planar, must match the playback rate in [SharedPlaybackState]
    pub data: Vec<Vec<f32>>,
    /// The stream index this frame belongs to
    pub stream_index: i32,
    /// Presentation timestamp
    pub pts: f64,
    /// Duration this frame should be shown
    pub duration: f64,
    /// Number of samples in [data]
    pub samples: usize,
}

#[derive(Clone)]
pub struct SubtitlePacket {
    pub data: Vec<u8>,
    pub stream_index: i32,
}

/// Container holding the channels for each media type
pub struct MediaStreams {
    pub metadata: Receiver<DecoderInfo>,
    pub video: Receiver<VideoFrame>,
    pub audio: Receiver<AudioSamples>,
    pub subtitle: Receiver<SubtitlePacket>,
}

/// Media stream producer, creates a stream of decoded data from a path or url.
/// To shut down the media stream you must drop the receiver channel(s)
pub struct MediaDecoder {
    /// Thread which decodes the media stream
    thread: JoinHandle<()>,
    /// Instance of the internal decoder
    internal: Box<dyn MediaDecoderImpl + 'static>,

    /// Internal shared data
    data: MediaDecoderThreadData,
}

/// Data shared with the decoder thread including decoder controls
#[derive(Debug, Clone)]
pub struct MediaDecoderThreadData {
    pub path: String,

    pub playback: SharedPlaybackState,

    // channels to send data back
    pub tx_m: SyncSender<DecoderInfo>,
    pub tx_v: SyncSender<VideoFrame>,
    pub tx_a: SyncSender<AudioSamples>,
    pub tx_s: SyncSender<SubtitlePacket>,
}

pub trait MediaDecoderImpl {
    /// Start the decoder thread
    fn start(&mut self) -> Result<JoinHandle<()>>;
}

impl MediaDecoder {
    /// Creates a new media player stream and returns the receiver channel
    pub fn new(input: &str, state: SharedPlaybackState) -> Result<(Self, MediaStreams)> {
        let (tx_m, rx_m) = sync_channel(1);
        let (tx_v, rx_v) = sync_channel(10);
        let (tx_a, rx_a) = sync_channel(1_000);
        let (tx_s, rx_s) = sync_channel(10);

        let thread_data = MediaDecoderThreadData {
            path: input.to_string(),
            playback: state,
            tx_m,
            tx_v,
            tx_a,
            tx_s,
        };
        let mut internal = Self::create_decoder(thread_data.clone())?;
        let thread = internal.start()?;
        Ok((
            Self {
                thread,
                internal,
                data: thread_data,
            },
            MediaStreams {
                metadata: rx_m,
                video: rx_v,
                audio: rx_a,
                subtitle: rx_s,
            },
        ))
    }

    #[allow(unused_variables)]
    fn create_decoder(data: MediaDecoderThreadData) -> Result<Box<dyn MediaDecoderImpl>> {
        #[cfg(feature = "ffmpeg")]
        return Ok(Box::new(ffmpeg::FfmpegDecoder::new(data)));
        #[cfg(feature = "avfoundation")]
        return Ok(Box::new(avfoundation::AvFoundationDecoder::new(data)));
        bail!("No decoder impl available!")
    }
}
