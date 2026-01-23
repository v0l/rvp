use crate::stream::AudioSamples;
use crate::{PlayerState, SharedPlaybackState};
use anyhow::bail;
use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig, StreamInstant};
use log::{error, info};
use scaletempo2::{
    mp_scaletempo2, mp_scaletempo2_create, mp_scaletempo2_fill_input_buffer,
    mp_scaletempo2_get_default_opts,
};
use std::collections::VecDeque;
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
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
            "Default audio device config: {} {}Hz, {}ch, {:?}",
            device.0.description()?.name(),
            cfg.sample_rate(),
            cfg.channels(),
            cfg.sample_format(),
        );

        let channels = cfg.channels() as u8;
        let sample_rate = cfg.sample_rate() as u32;
        let mut first_frame = false;

        // update the playback state with the audio device playback details
        p.sample_rate.store(sample_rate, Ordering::Relaxed);
        p.channels.store(channels, Ordering::Relaxed);

        let mut simple_queue = Vec::new();
        for _ in 0..channels {
            simple_queue.push(VecDeque::new());
        }
        let mut audio_scale = AudioScale::new(channels, sample_rate).expect("audio scale");
        let stream = device.0.build_output_stream_raw(
            &cfg.config(),
            SampleFormat::F32,
            move |data: &mut cpal::Data, info: &cpal::OutputCallbackInfo| {
                if data.len() == 0 {
                    return;
                }
                let dst: &mut [f32] = data.as_slice_mut().unwrap();
                dst.fill(0.0);
                let state = p.state();
                if state == PlayerState::Stopped || state == PlayerState::Paused {
                    return;
                }
                // number of samples per channel to drain
                let stride = dst.len() / channels as usize;

                if stride == 0 {
                    panic!("Nothing to drain");
                }

                // fill queue until dst is satisfied
                while simple_queue[0].len() < stride {
                    // take samples from channel
                    match rx.try_recv() {
                        Ok(m) => {
                            // for the first frame set the queue head pts
                            if !first_frame {
                                first_frame = true;
                                let buffer_delay = info
                                    .timestamp()
                                    .playback
                                    .duration_since(&info.timestamp().callback)
                                    .unwrap_or(Duration::ZERO)
                                    .as_secs_f64();
                                info!("First audio frame pts={}, delay={}", m.pts, buffer_delay);
                                p.incr_audio_pts(buffer_delay);
                            }
                            for (chan, data) in m.data.into_iter().enumerate() {
                                simple_queue[chan].extend(data);
                            }
                        }
                        Err(mpsc::TryRecvError::Empty) => {
                            continue;
                        }
                        Err(_) => {
                            break;
                        }
                    }
                }
                let mut in_samples = simple_queue
                    .iter_mut()
                    .map(|r| r.drain(..stride).collect::<Vec<_>>())
                    .collect::<Vec<_>>();

                // move queue head pts
                let drain_samples_pts = stride as f64 / sample_rate as f64;
                p.incr_audio_pts(drain_samples_pts);

                // after draining all the samples, drop them
                if p.muted() {
                    return;
                }

                let speed = p.speed();
                let volume = p.volume();
                if speed != 1.0 {
                    let dst_samples = dst.len() / channels as usize;
                    // create a buffer to hold the output samples
                    // device samples are always packed
                    let mut out_samples = Vec::with_capacity(channels as usize);
                    out_samples.resize_with(channels as _, || {
                        let mut v_line = Vec::with_capacity(dst_samples);
                        v_line.resize(dst_samples, 0.0);
                        v_line
                    });

                    todo!();
                } else {
                    let chans = in_samples.len();
                    for (x, chan) in in_samples.iter_mut().enumerate() {
                        for z in 0..stride {
                            dst[x + (chans * z)] = chan[z] * volume;
                        }
                    }
                }
            },
            move |e| {
                error!("{}", e);
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

struct AudioScale {
    ctx: NonNull<mp_scaletempo2>,
    _m: PhantomData<mp_scaletempo2>,
}

impl AudioScale {
    pub fn new(channels: u8, sample_rate: u32) -> Result<AudioScale> {
        unsafe {
            let opts = mp_scaletempo2_get_default_opts();
            let ctx = mp_scaletempo2_create(&opts, channels as _, sample_rate as _);
            if ctx.is_null() {
                bail!("Failed to create default audio device");
            }
            Ok(Self {
                ctx: NonNull::new(ctx).context("failed to create audio device")?,
                _m: PhantomData,
            })
        }
    }

    pub fn process(
        &mut self,
        in_samples: Vec<Vec<f32>>,
        in_size: usize,
        out_samples: Vec<Vec<f32>>,
        speed: f64,
    ) {
        let mut in_ptrs = in_samples.iter().map(|s| s.as_ptr()).collect::<Vec<_>>();
        unsafe {
            let proc_samples = mp_scaletempo2_fill_input_buffer(
                self.ctx.as_mut(),
                in_ptrs.as_mut_ptr() as _,
                in_size as _,
                speed,
            );
        }
    }
}
