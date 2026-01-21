use crate::stream::{
    AudioSamples, DecoderInfo, MediaDecoderImpl, MediaDecoderThreadData, StreamInfo,
    SubtitlePacket, VideoFrame,
};
use anyhow::{Result, bail};
use egui::{Color32, ColorImage, Vec2};
use ffmpeg_rs_raw::ffmpeg_sys_the_third::{
    AV_NOPTS_VALUE, AVMediaType, AVPixelFormat, AVSampleFormat, av_get_pix_fmt_name,
    av_get_sample_fmt_name, av_q2d, avcodec_get_name,
};
use ffmpeg_rs_raw::{
    AvFrameRef, AvPacketRef, Decoder, Demuxer, DemuxerInfo, Resample, Scaler, StreamType,
    get_frame_from_hw, rstr,
};
use log::error;
use std::mem::transmute;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;

fn video_frame_to_image(frame: &AvFrameRef) -> ColorImage {
    let pixels: Vec<Color32> = match unsafe { transmute(frame.format) } {
        AVPixelFormat::AV_PIX_FMT_RGB24 | AVPixelFormat::AV_PIX_FMT_RGBA => {
            map_frame_to_pixels(frame)
        }
        _ => panic!("Pixel format not supported!"),
    };
    ColorImage {
        source_size: Vec2::new(frame.width as _, frame.height as _),
        size: [frame.width as _, frame.height as _],
        pixels,
    }
}

fn map_frame_to_pixels(frame: &AvFrameRef) -> Vec<Color32> {
    let stride = frame.linesize[0] as usize;
    let lines = frame.height as usize;
    let data = unsafe { std::slice::from_raw_parts_mut((*frame).data[0], stride * lines) };
    let bytes = match unsafe { transmute((*frame).format) } {
        AVPixelFormat::AV_PIX_FMT_RGB24 => 3,
        AVPixelFormat::AV_PIX_FMT_RGBA => 4,
        _ => panic!("Pixel format not supported!"),
    };
    (0..lines)
        .map(|r| {
            let offset = r * stride;
            data[offset..offset + stride]
                .chunks_exact(bytes)
                .take((*frame).width as usize)
                .map(|c| match bytes {
                    3 => Color32::from_rgb(c[0], c[1], c[2]),
                    4 => Color32::from_rgba_premultiplied(c[0], c[1], c[2], c[3]),
                    _ => panic!("not possible"),
                })
        })
        .flatten()
        .collect()
}

/// Internal FFMPEG decoder thread instance
struct DecoderThread {
    data: MediaDecoderThreadData,
    demuxer: Demuxer,
    decoder: Decoder,
    scaler: Scaler,
    resample: Resample,
    info: Option<DemuxerInfo>,
}

impl DecoderThread {
    fn tick(&mut self) -> Result<()> {
        if self.info.is_none() {
            self.probe()?;
        }

        let (pkt, _) = unsafe { self.demuxer.get_packet()? };
        let v_index = self.data.selected_video.load(Ordering::Relaxed);
        let a_index = self.data.selected_audio.load(Ordering::Relaxed);
        // let s_index = self.data.selected_subtitle.load(Ordering::Relaxed);
        if let Some(pkt) = pkt.as_ref()
            && !(pkt.stream_index == v_index as _ || pkt.stream_index == a_index as _)
        {
            // skip packet, not playing
            return Ok(());
        }
        if pkt.is_none() {
            bail!("Stream ended (EOF)");
        }

        Ok(())
    }

    fn decode_packet(&mut self, pkt: Option<AvPacketRef>) -> Result<()> {
        let frames = self.decoder.decode_pkt(pkt.as_ref())?;
        for (frame, stream_index) in frames {
            let stream = unsafe { self.demuxer.get_stream(stream_index as _)? };
            let frame = get_frame_from_hw(frame)?;
            let q = unsafe { av_q2d((*stream).time_base) };
            match unsafe { (*(*stream).codecpar).codec_type } {
                AVMediaType::AVMEDIA_TYPE_VIDEO => {
                    self.send_video(frame, stream_index, q)?;
                }
                AVMediaType::AVMEDIA_TYPE_AUDIO => {
                    self.send_audio(frame, stream_index, q)?;
                }
                AVMediaType::AVMEDIA_TYPE_SUBTITLE => {
                    self.send_subtitle(frame, stream_index, q)?;
                }
                _ => continue,
            }
        }
        Ok(())
    }

    fn send_video(&mut self, frame: AvFrameRef, stream_index: i32, q: f64) -> Result<()> {
        // convert to RBGA
        let new_frame = self.scaler.process_frame(
            &frame,
            frame.width as _,
            frame.height as _,
            AVPixelFormat::AV_PIX_FMT_RGBA,
        )?;
        self.data.tx_v.send(VideoFrame {
            data: video_frame_to_image(&new_frame),
            stream_index,
            pts: if frame.pts != AV_NOPTS_VALUE {
                frame.pts as f64 * q
            } else {
                0.0
            },
            duration: if frame.duration != AV_NOPTS_VALUE {
                frame.duration as f64 * q
            } else {
                0.0
            },
        })?;
        Ok(())
    }

    fn send_audio(&mut self, frame: AvFrameRef, stream_index: i32, q: f64) -> Result<()> {
        let frame = self.resample.process_frame(&frame)?;
        self.data.tx_a.send(AudioSamples {
            data: unsafe {
                // TODO: check alignment
                std::slice::from_raw_parts(
                    frame.data[0] as *mut _,
                    (frame.nb_samples * frame.ch_layout.nb_channels) as usize,
                )
            }
            .to_vec(),
            samples: frame.nb_samples as usize,
            stream_index,
            pts: if frame.pts != AV_NOPTS_VALUE {
                frame.pts as f64 * q
            } else {
                0.0
            },
            duration: if frame.duration != AV_NOPTS_VALUE {
                frame.duration as f64 * q
            } else {
                0.0
            },
        })?;
        Ok(())
    }

    fn send_subtitle(&mut self, _frame: AvFrameRef, stream_index: i32, _q: f64) -> Result<()> {
        self.data.tx_s.send(SubtitlePacket {
            data: vec![],
            stream_index,
        })?;
        Ok(())
    }

    fn probe(&mut self) -> Result<()> {
        let probe = unsafe { self.demuxer.probe_input()? };
        self.info.replace(probe.clone());

        // pick the best video/audio/subtitle stream
        let pick_video = probe
            .streams
            .iter()
            .filter(|s| s.stream_type == StreamType::Video)
            .max_by_key(|s| s.width * s.height)
            .map(|s| s.index as isize)
            .unwrap_or(-1);
        let pick_audio = probe
            .streams
            .iter()
            .filter(|s| s.stream_type == StreamType::Audio)
            .max_by_key(|s| s.bitrate)
            .map(|s| s.index as isize)
            .unwrap_or(-1);
        let pick_subtitle = probe
            .streams
            .iter()
            .filter(|s| s.stream_type == StreamType::Subtitle)
            .next()
            .map(|s| s.index as isize)
            .unwrap_or(-1);
        self.data
            .selected_video
            .store(pick_video, Ordering::Relaxed);
        self.data
            .selected_audio
            .store(pick_audio, Ordering::Relaxed);
        self.data
            .selected_subtitle
            .store(pick_subtitle, Ordering::Relaxed);

        for stream in probe.streams.iter() {
            if stream.index == pick_video as _
                || stream.index == pick_audio as _
                || stream.index == pick_subtitle as _
            {
                self.decoder.setup_decoder(stream, None)?;
            }
        }

        let inf = DecoderInfo {
            bitrate: probe.bitrate as _,
            duration: probe.duration,
            streams: probe
                .streams
                .iter()
                .filter_map(|s| {
                    Some(StreamInfo {
                        r#type: match s.stream_type {
                            StreamType::Unknown => return None,
                            StreamType::Video => crate::stream::StreamType::Video,
                            StreamType::Audio => crate::stream::StreamType::Audio,
                            StreamType::Subtitle => crate::stream::StreamType::Subtitle,
                        },
                        index: s.index as _,
                        codec: unsafe {
                            if let Some(dec) = self.decoder.get_decoder(s.index as _) {
                                dec.codec_name()
                            } else {
                                let n = avcodec_get_name(transmute(s.codec as i32));
                                rstr!(n).to_string()
                            }
                        },
                        format: unsafe {
                            if s.width != 0 {
                                let n = av_get_pix_fmt_name(transmute(s.format as i32));
                                rstr!(n).to_string()
                            } else {
                                let n = av_get_sample_fmt_name(transmute(s.format as i32));
                                rstr!(n).to_string()
                            }
                        },
                        channels: s.channels,
                        sample_rate: s.sample_rate as _,
                        width: s.width as _,
                        height: s.height as _,
                        fps: s.fps,
                        language: if s.language.is_empty() {
                            None
                        } else {
                            Some(s.language.clone())
                        },
                    })
                })
                .collect(),
        };

        self.data.tx_m.send(inf)?;
        Ok(())
    }
}

pub(crate) struct FfmpegDecoderImpl {
    data: MediaDecoderThreadData,
}

impl FfmpegDecoderImpl {
    pub(crate) fn new(data: MediaDecoderThreadData) -> Self {
        Self { data }
    }
}

impl MediaDecoderImpl for FfmpegDecoderImpl {
    fn start(&mut self) -> Result<JoinHandle<()>> {
        let mut instance = DecoderThread {
            data: self.data.clone(),
            demuxer: Demuxer::new(&self.data.path)?,
            decoder: Decoder::new(),
            scaler: Scaler::new(),
            resample: Resample::new(
                AVSampleFormat::AV_SAMPLE_FMT_S32,
                self.data.sample_rate.load(Ordering::Relaxed),
                self.data.channels.load(Ordering::Relaxed) as _,
            ),
            info: None,
        };
        Ok(std::thread::Builder::new()
            .name("media-decoder-ffmpeg".to_string())
            .spawn(move || {
                instance.decoder.enable_hw_decoder_any();
                loop {
                    if let Err(e) = instance.tick() {
                        error!("{}", e);
                        break;
                    }
                }
            })?)
    }
}
