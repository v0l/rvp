use crate::stream::{
    DecoderInfo, MediaDecoderImpl, MediaDecoderThreadData, StreamInfo, StreamType,
};
use anyhow::Result;
use log::{error, info};
use objc2::rc::Retained;
use objc2_av_foundation::AVURLAsset;
use objc2_foundation::{NSString, NSURL};
use std::thread::JoinHandle;

/// Internal AVFoundation decoder thread instance
struct DecoderThread {
    data: MediaDecoderThreadData,
    asset: Retained<AVURLAsset>,
    info: Option<DecoderInfo>,
}

impl DecoderThread {
    fn tick(&mut self) -> Result<()> {
        if self.info.is_none() {
            unsafe {
                self.probe()?;
            }
        }
        Ok(())
    }

    unsafe fn probe(&mut self) -> Result<()> {
        let tracks = unsafe { self.asset.tracks() };
        let metadata = unsafe { self.asset.commonMetadata() };
        info!("{:?}", metadata);
        info!("{:?}", tracks);
        self.info.replace(DecoderInfo {
            bitrate: tracks
                .iter()
                .map(|t| unsafe { t.estimatedDataRate() } as u64)
                .sum::<u64>(),
            duration: metadata
                .iter()
                .find_map(|d| {
                    let dur = unsafe { d.duration() };
                    if dur.value as i64 != 0 {
                        Some(dur.value as f32 / dur.timescale as f32)
                    } else {
                        None
                    }
                })
                .unwrap_or(0.0),
            streams: tracks
                .iter()
                .filter_map(|t| {
                    let mt = unsafe { t.mediaType().to_string() };
                    let format = unsafe { t.formatDescriptions() };
                    match mt.as_str() {
                        "vide" => {
                            let size = unsafe { t.naturalSize() };
                            let fps = unsafe { t.nominalFrameRate() };
                            Some(StreamInfo {
                                r#type: StreamType::Video,
                                index: unsafe { t.trackID() },
                                codec: format
                                    .firstObject()
                                    .map(|o| format!("{:?}", o))
                                    .unwrap_or(String::new()),
                                format: "".to_string(),
                                channels: 0,
                                sample_rate: 0,
                                width: size.width as _,
                                height: size.height as _,
                                fps: fps as _,
                                language: None,
                            })
                        },
                        "soun" => {
                            let lang = unsafe { t.languageCode() };
                            Some(StreamInfo {
                                r#type: StreamType::Audio,
                                index: unsafe { t.trackID() },
                                codec: format
                                    .firstObject()
                                    .map(|o| format!("{:?}", o))
                                    .unwrap_or(String::new()),
                                format: "".to_string(),
                                channels: 0,
                                sample_rate: 0,
                                width: 0,
                                height: 0,
                                fps: 0.0,
                                language: lang.map(|l| l.to_string()),
                            })
                        },
                        _ => None,
                    }
                })
                .collect(),
        });
        Ok(())
    }
}

pub struct AvFoundationDecoder {
    data: MediaDecoderThreadData,
}

impl AvFoundationDecoder {
    pub fn new(data: MediaDecoderThreadData) -> Self {
        Self { data }
    }
}

impl MediaDecoderImpl for AvFoundationDecoder {
    fn start(&mut self) -> Result<JoinHandle<()>> {
        let mut instance = DecoderThread {
            data: self.data.clone(),
            asset: unsafe {
                AVURLAsset::assetWithURL(
                    NSURL::fileURLWithPath(NSString::from_str(&self.data.path).as_ref()).as_ref(),
                )
            },
            info: None,
        };
        Ok(std::thread::Builder::new()
            .name("media-decoder-av-foundation".to_string())
            .spawn(move || {
                loop {
                    if let Err(e) = instance.tick() {
                        error!("{}", e);
                        break;
                    }
                }
            })?)
    }
}
