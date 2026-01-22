use crate::stream::AudioSamples;
use crate::{PlayerState, SharedPlaybackState};
use anyhow::Result;
use anyhow::bail;
use bungee_sys::BungeeStream;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use log::{error, info, trace};
use std::collections::VecDeque;
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicI16, AtomicI64, AtomicU8, AtomicU16, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, mpsc};
use std::thread::sleep;
use std::time::Duration;

/// The playback device. Needs to be initialized (and kept alive!) for use by a [`Player`].
pub struct AudioDevice(pub(crate) cpal::Device);

/// Handle to an actively running audio stream
pub struct AudioDeviceHandle {
    device: AudioDevice,
    stream: Stream,
    config: StreamConfig,
}

impl crate::AudioDevice for AudioDeviceHandle {
    fn channels(&self) -> u8 {
        self.config.channels as _
    }

    fn sample_rate(&self) -> u32 {
        self.config.sample_rate as _
    }
}

impl AudioDevice {
    pub fn from_device(device: cpal::Device) -> Self {
        Self(device)
    }

    /// Create a new [`AudioDevice`] from an existing [`cpal::Host`]. An [`AudioDevice`] is required for using audio.
    pub fn from_subsystem(audio_sys: &cpal::Host) -> Result<AudioDevice> {
        if let Some(dev) = audio_sys.default_output_device() {
            Ok(AudioDevice(dev))
        } else {
            bail!("No default audio device available");
        }
    }

    /// Create a new [`AudioDevice`]. Creates an [`cpal::Host`]. An [`AudioDevice`] is required for using audio.
    pub fn new() -> Result<AudioDevice> {
        let host = cpal::default_host();
        Self::from_subsystem(&host)
    }

    pub fn open_default_audio_stream(
        p: SharedPlaybackState,
        rx: Receiver<AudioSamples>,
    ) -> Result<AudioDeviceHandle> {
        let device = AudioDevice::new()?;
        let cfg = device.0.default_output_config()?;
        info!(
            "Default audio device config: {}Hz, {}ch, {:?}",
            cfg.sample_rate(),
            cfg.channels(),
            cfg.sample_format()
        );

        let channels = cfg.channels() as u8;
        let sample_rate = cfg.sample_rate() as u32;
        let mut first_frame = false;

        let mut simple_queue = Vec::new();
        for _ in 0..channels {
            simple_queue.push(VecDeque::new());
        }
        let mut queue_head_pts = 0.0;
        let mut bungee_stream = BungeeWrapper::new(channels, sample_rate)?;
        let stream = device.0.build_output_stream_raw(
            &cfg.config(),
            SampleFormat::F32,
            move |data: &mut cpal::Data, _info: &cpal::OutputCallbackInfo| {
                if data.len() == 0 {
                    return;
                }
                let dst: &mut [f32] = data.as_slice_mut().unwrap();
                dst.fill(0.0);
                let state = p.state.load(Ordering::Relaxed);
                if state == PlayerState::Stopped as u8 || state == PlayerState::Paused as u8 {
                    return;
                }
                let current_pts = p.pts.load(Ordering::Relaxed) as f64 / 1000.0;

                /// number of samples to stretch to move towards pts target (per channel)
                const NUDGE_SAMPLES: usize = 128;

                let nudge_drift = NUDGE_SAMPLES as f64 / sample_rate as f64;
                let pts_drift = queue_head_pts - current_pts;
                let drain_samples = if pts_drift < nudge_drift {
                    // drain more samples for the input side
                    dst.len() + NUDGE_SAMPLES
                } else {
                    dst.len() - NUDGE_SAMPLES
                };

                let mut empty_loop = 0;
                // fill queue until dst is satisfied
                while simple_queue[0].len() < drain_samples {
                    // take samples from channel
                    match rx.try_recv() {
                        Ok(m) => {
                            // for the first frame set the queue head pts
                            if !first_frame {
                                first_frame = true;
                                queue_head_pts = m.pts;
                            }
                            for (chan, data) in m.data.into_iter().enumerate() {
                                simple_queue[chan].extend(data);
                            }
                        }
                        Err(mpsc::TryRecvError::Empty) => {
                            // max wait 50ms
                            if empty_loop > 10 {
                                error!("Audio underrun!");
                                return;
                            }
                            trace!("Audio underrun!");
                            empty_loop += 1;
                            sleep(Duration::from_millis(5));
                            continue;
                        }
                        Err(_) => {
                            break;
                        }
                    }
                }
                let in_samples = simple_queue
                    .iter_mut()
                    .map(|r| r.drain(..drain_samples).collect::<Vec<_>>())
                    .collect::<Vec<_>>();

                let out_samples_len = dst.len() / channels as usize;
                let mut out_samples = Vec::with_capacity(channels as usize);
                out_samples.fill(Vec::with_capacity(out_samples_len));

                if let Err(e) = bungee_stream.process(
                    in_samples,
                    drain_samples,
                    &mut out_samples,
                    out_samples_len,
                ) {
                    panic!("Error processing audio data: {}", e);
                }

                // move queue head pts
                let head_drain = out_samples_len as f64 / sample_rate as f64;
                queue_head_pts += head_drain;
            },
            move |e| {
                panic!("{}", e);
            },
            None,
        )?;
        stream.play()?;
        Ok(AudioDeviceHandle {
            device,
            stream,
            config: cfg.config(),
        })
    }
}

pub struct BungeeWrapper {
    ctx: NonNull<BungeeStream>,
    _marker: PhantomData<BungeeStream>,
}

unsafe impl Send for BungeeWrapper {}

impl BungeeWrapper {
    pub fn new(channels: u8, sample_rate: u32) -> Result<BungeeWrapper> {
        let ctx = {
            let samples = bungee_sys::SampleRates {
                input: sample_rate as _,
                output: sample_rate as _,
            };
            let stretcher = bungee_sys::stretcher::create(samples, channels as _, 2);
            if stretcher.is_null() {
                bail!("Failed to create stretcher");
            }
            let ctx = bungee_sys::stream::create(stretcher, channels as _, 8192);
            if ctx.is_null() {
                bail!("Failed to create stretcher stream");
            }
            ctx
        };
        Ok(Self {
            ctx: NonNull::new(ctx).unwrap(),
            _marker: PhantomData,
        })
    }

    pub fn process(
        &mut self,
        in_data: Vec<Vec<f32>>,
        in_samples: usize,
        out_data: &mut Vec<Vec<f32>>,
        out_samples: usize,
    ) -> Result<()> {
        let ret = bungee_sys::stream::process(
            self.ctx.as_ptr(),
            in_data.as_ptr() as *const *const f32,
            out_data.as_mut_ptr() as *mut *mut _,
            in_samples as _,
            out_samples as _,
            1.0,
        );
        if ret != 0 {
            bail!("Failed to process audio");
        }
        Ok(())
    }
}
