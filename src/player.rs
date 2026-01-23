use crate::stream::{
    AudioSamples, DecoderInfo, MediaDecoder, StreamInfo, SubtitlePacket, VideoFrame,
};
#[cfg(feature = "subtitles")]
use crate::subtitle::Subtitle;
use crate::{AudioDevice, NoAudioDevice, SharedPlaybackState, format_time};
use anyhow::Result;
use egui::load::SizedTexture;
use egui::text::LayoutJob;
use egui::{
    Align2, Color32, ColorImage, Event, FontId, Image, Key, Rect, Response, Sense, Stroke,
    StrokeKind, TextFormat, TextureHandle, TextureOptions, Ui, Vec2, Widget, pos2, vec2,
};
use log::{info, trace};
use std::fmt::Display;
use std::ops::Add;
use std::sync::Arc;
use std::sync::atomic::{
    AtomicBool, AtomicI8, AtomicI64, AtomicIsize, AtomicU8, AtomicU16, AtomicU32, AtomicU64,
    Ordering,
};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

#[cfg(not(feature = "subtitles"))]
struct Subtitle;

/// Generic overlay for player controls
pub trait PlayerOverlay: Send {
    /// Show the overlay
    fn show(&self, ui: &mut Ui, frame_response: &Response, p: &SharedPlaybackState);
}

struct NoOverlay;
impl PlayerOverlay for NoOverlay {
    fn show(&self, _ui: &mut Ui, _frame_response: &Response, _p: &SharedPlaybackState) {
        // noop
    }
}

/// The [`Player`] processes and controls streams of video/audio.
/// This is what you use to show a video file.
/// Initialize once, and use the [`Player::ui`] or [`Player::ui_at()`] functions to show the playback.
pub struct Player {
    overlay: Box<dyn PlayerOverlay>,
    state: SharedPlaybackState,
    debug: bool,

    avg_fps: f32,
    avg_fps_start: Instant,
    last_frame_counter: u64,

    /// The video frame to display
    frame: TextureHandle,
    /// Start presentation time for the current frame
    frame_pts: f64,
    /// Length to show this frame in seconds
    frame_duration: f64,
    /// Clock time when the frame began
    frame_instant: Instant,

    /// How many frames have been rendered so far
    frame_counter: u64,
    /// Maintain video aspect ratio
    maintain_aspect: bool,
    /// If player should fullscreen
    fullscreen: bool,
    /// If key presses should be handled
    key_binds: bool,

    /// Stream info
    stream_info: Option<DecoderInfo>,

    ctx: egui::Context,
    input_path: String,
    audio: Box<dyn AudioDevice>,
    subtitle: Option<Subtitle>,

    /// Media stream decoder thread
    media_player: MediaDecoder,
    rx_metadata: Receiver<DecoderInfo>,
    rx_video: Receiver<VideoFrame>,
    rx_subtitle: Receiver<SubtitlePacket>,

    /// An error which prevented playback
    error: Option<String>,

    /// Message to show on scree for a short time (usually from keyboard input)
    osd: Option<String>,
    osd_end: Instant,
}

/// The possible states of a [`Player`].
#[derive(PartialEq, Clone, Copy, Debug)]
#[repr(u8)]
#[non_exhaustive]
pub enum PlayerState {
    /// No playback.
    Stopped,
    /// Stream is seeking. Inner bool represents whether the seek is currently in progress.
    Seeking,
    /// Playback is paused.
    Paused,
    /// Playback is ongoing.
    Playing,
}

impl From<u8> for PlayerState {
    fn from(value: u8) -> Self {
        match value {
            0 => PlayerState::Stopped,
            1 => PlayerState::Seeking,
            2 => PlayerState::Paused,
            3 => PlayerState::Playing,
            _ => PlayerState::Stopped,
        }
    }
}

impl Display for PlayerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlayerState::Stopped => write!(f, "Stopped"),
            PlayerState::Seeking => write!(f, "Seeking"),
            PlayerState::Paused => write!(f, "Paused"),
            PlayerState::Playing => write!(f, "Playing"),
        }
    }
}

impl Player {
    /// Store the next image
    fn load_frame(&mut self, frame: VideoFrame) {
        trace!(
            "Loading video frame idx={}, pts={}, dur={}",
            self.frame_counter, frame.pts, frame.duration
        );
        self.frame.set(frame.data, TextureOptions::default());
        self.frame_pts = frame.pts;
        self.frame_duration = frame.duration;
        self.frame_counter += 1;
        self.frame_instant = Instant::now();
        self.state.set_video_pts(frame.pts);

        // apply playback speed by adjusting frame duration
        let speed = self.state.speed() as f64 / 1.0;
        //self.frame_duration *= speed;

        // tweak duration for A/V sync
        let diff = self.state.video_pts() - self.state.audio_pts();
        let max_diff = self.frame_duration * 0.1;
        //self.frame_duration += diff.clamp(-max_diff, max_diff);
    }

    fn request_repaint_for_next_frame(&self) {
        let now = Instant::now();
        let next_frame = self
            .frame_instant
            .add(Duration::from_secs_f64(self.frame_duration));
        if now > next_frame {
            self.ctx.request_repaint();
        } else {
            let tt_nf = next_frame - now;
            self.ctx.request_repaint_after(tt_nf);
        }
    }

    /// Check if the current frame should be flipped
    fn check_load_frame(&mut self) -> bool {
        if self.state.state() == PlayerState::Paused {
            // force frame to start now, while paused
            self.frame_instant = Instant::now();
            return false;
        }

        let now = Instant::now();
        now >= self.frame_end_instant()
    }

    /// Instant when the current frame ends
    fn frame_end_instant(&self) -> Instant {
        self.frame_instant
            .add(Duration::from_secs_f64(self.frame_duration))
    }

    /// Enable/Disable built-in keybind controls
    pub fn enable_keybinds(&mut self, v: bool) {
        self.key_binds = v;
    }

    /// Handle key input
    fn handle_keys(&mut self, ui: &mut Ui) {
        const SEEK_STEP: f32 = 5.0;
        const VOLUME_STEP: f32 = 0.01;
        const SPEED_STEP: f32 = 0.1;

        if !self.key_binds {
            return;
        }

        ui.input(|inputs| {
            for e in &inputs.events {
                match e {
                    Event::Key { key, pressed, .. } if *pressed => match key {
                        Key::Space => {
                            if self.state.state() == PlayerState::Playing {
                                self.state.set_state(PlayerState::Paused);
                            } else {
                                self.state.set_state(PlayerState::Playing);
                            }
                        }
                        Key::OpenBracket => {
                            self.state.decr_speed(SPEED_STEP);
                        }
                        Key::CloseBracket => {
                            self.state.incr_speed(SPEED_STEP);
                        }
                        Key::ArrowRight => {
                            // not implemented
                        }
                        Key::ArrowLeft => {
                            // not implemented
                        }
                        Key::ArrowUp => {
                            self.state.incr_volume(VOLUME_STEP);
                        }
                        Key::ArrowDown => {
                            self.state.decr_volume(VOLUME_STEP);
                        }
                        Key::F => {
                            self.fullscreen = !self.fullscreen;
                        }
                        Key::F1 => {
                            self.debug = !self.debug;
                        }
                        Key::M => {
                            self.state.set_muted(!self.state.muted());
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
        });
    }

    fn process_state(&mut self) {
        let current_state = self.state.state();
        if self.stream_info.is_none()
            && let Ok(md) = self.rx_metadata.try_recv()
        {
            self.state.set_duration(md.duration as _);
            self.stream_info.replace(md);
            if current_state != PlayerState::Playing {
                self.state.set_state(PlayerState::Playing);
            }
        }

        if current_state == PlayerState::Stopped {
            // nothing to do, playback is stopped
            return;
        }

        // check if we should load the next video frame
        if !self.check_load_frame() {
            self.request_repaint_for_next_frame();
            return;
        }

        // reset avg fps every 1s
        let n_frames = self.frame_counter - self.last_frame_counter;
        if n_frames >= 30 {
            self.avg_fps = n_frames as f32 / (Instant::now() - self.avg_fps_start).as_secs_f32();
            self.avg_fps_start = Instant::now();
            self.last_frame_counter = self.frame_counter;
        }

        if let Ok(msg) = self.rx_video.recv() {
            self.load_frame(msg);
            // break on video frame
            // once we load the next frame this loop will not call again until
            // this frame is over (pts + duration)
            self.request_repaint_for_next_frame();
            return;
        }

        // if no frames were found just request repaint again
        self.request_repaint_for_next_frame();
    }

    fn generate_frame_image(&self, size: Vec2) -> Image<'_> {
        Image::new(SizedTexture::new(self.frame.id(), size)).sense(Sense::click())
    }

    fn render_frame(&self, ui: &mut Ui) -> Response {
        self.render_frame_at(ui, ui.available_rect_before_wrap())
    }

    /// Exact size of the video frame inside a given [Rect]
    fn video_frame_size(&self, rect: Rect) -> Vec2 {
        if self.maintain_aspect {
            let bv = self.current_video_stream();
            let video_size = bv
                .map(|v| vec2(v.width as f32, v.height as f32))
                .unwrap_or(rect.size());
            let ratio = video_size.x / video_size.y;
            let rect_ratio = rect.width() / rect.height();
            if ratio > rect_ratio {
                let h = rect.width() / ratio;
                vec2(rect.width().floor(), h.floor())
            } else if ratio < rect_ratio {
                let w = rect.height() * ratio;
                vec2(w.floor(), rect.height().floor())
            } else {
                rect.size()
            }
        } else {
            rect.size()
        }
    }

    fn render_frame_at(&self, ui: &mut Ui, rect: Rect) -> Response {
        let video_size = self.video_frame_size(rect);
        ui.painter()
            .rect(rect, 0.0, Color32::BLACK, Stroke::NONE, StrokeKind::Middle);
        ui.put(rect, self.generate_frame_image(video_size))
    }

    fn render_subtitles(&mut self, _ui: &mut Ui) {
        #[cfg(feature = "subtitles")]
        if let Some(s) = self.subtitle.as_ref() {
            let sub_end = s.pts + s.duration;
            if sub_end < self.current_pts() {
                self.subtitle.take();
            } else {
                ui.add(s);
            }
        }
    }

    fn render_debug(&mut self, ui: &mut Ui, frame_response: &Response) {
        let painter = ui.painter();

        const PADDING: f32 = 5.0;
        let vec_padding = vec2(PADDING, PADDING);
        let job = self.debug_inner(frame_response.rect);
        let galley = painter.layout_job(job);
        let mut bg_pos = galley
            .rect
            .translate(frame_response.rect.min.to_vec2() + vec_padding);
        bg_pos.max += vec_padding * 2.0;
        painter.rect_filled(bg_pos, PADDING, Color32::from_black_alpha(150));
        painter.galley(bg_pos.min + vec_padding, galley, Color32::PLACEHOLDER);
    }

    fn show_osd(&mut self, msg: &str) {
        self.osd = Some(msg.to_string());
        self.osd_end = Instant::now() + Duration::from_secs(2);
    }

    /// Get the currently playing video stream info
    fn current_video_stream(&self) -> Option<&StreamInfo> {
        if let Some(i) = self.stream_info.as_ref() {
            let v_index = self.state.selected_video.load(Ordering::Relaxed);
            i.streams.iter().find(|s| s.index == v_index as _)
        } else {
            None
        }
    }

    /// Get the currently playing audio stream info
    fn current_audio_stream(&self) -> Option<&StreamInfo> {
        if let Some(i) = self.stream_info.as_ref() {
            let v_index = self.state.selected_audio.load(Ordering::Relaxed);
            i.streams.iter().find(|s| s.index == v_index as _)
        } else {
            None
        }
    }

    /// Get the currently playing subtitle stream info
    fn current_subtitle_stream(&self) -> Option<&StreamInfo> {
        if let Some(i) = self.stream_info.as_ref() {
            let v_index = self.state.selected_subtitle.load(Ordering::Relaxed);
            i.streams.iter().find(|s| s.index == v_index as _)
        } else {
            None
        }
    }

    fn debug_inner(&mut self, frame_response: Rect) -> LayoutJob {
        let font = TextFormat::simple(FontId::monospace(11.), Color32::WHITE);

        let mut layout = LayoutJob::default();
        let v_pts = self.state.video_pts();
        let a_pts = self.state.audio_pts();

        layout.append(
            &format!(
                "sync: v:{:.3}s, a:{:.3}s, a-sync:{:.3}s",
                v_pts,
                a_pts,
                a_pts - v_pts,
            ),
            0.0,
            font.clone(),
        );

        let video_stream = self.current_video_stream();
        let video_size = self.video_frame_size(frame_response);
        layout.append(
            &format!(
                "\nplayback: {:.2} fps ({:.2}x), volume={:.0}%, resolution={}x{}",
                self.avg_fps,
                self.avg_fps / video_stream.map(|s| s.fps).unwrap_or(1.0),
                100.0 * (self.state.volume()),
                video_size.x,
                video_size.y
            ),
            0.0,
            font.clone(),
        );

        if let Some(info) = self.stream_info.as_ref() {
            let bitrate_str = if info.bitrate > 1_000_000 {
                format!("{:.1}M", info.bitrate as f32 / 1_000_000.0)
            } else if info.bitrate > 1_000 {
                format!("{:.1}k", info.bitrate as f32 / 1_000.0)
            } else {
                info.bitrate.to_string()
            };

            layout.append(
                &format!(
                    "\nduration: {}, bitrate: {}",
                    format_time(info.duration),
                    bitrate_str
                ),
                0.0,
                font.clone(),
            );

            fn print_chan(layout: &mut LayoutJob, font: TextFormat, chan: Option<&StreamInfo>) {
                if let Some(c) = chan {
                    layout.append(&format!("\n  {}", c), 0.0, font.clone());
                }
            }
            print_chan(&mut layout, font.clone(), video_stream);
            print_chan(&mut layout, font.clone(), self.current_audio_stream());
            print_chan(&mut layout, font.clone(), self.current_subtitle_stream());
        }

        layout
    }

    /// Create a new [`Player`].
    pub fn new(ctx: &egui::Context, input_path: &str) -> Result<Self> {
        let state = SharedPlaybackState::new();

        let (media_player, streams) =
            MediaDecoder::new(input_path, state.clone()).expect("Failed to create media playback");

        let audio = Self::open_audio(state.clone(), streams.audio)?;

        let init_size = ctx.available_rect();
        Ok(Self {
            state,
            overlay: Box::new(NoOverlay),
            key_binds: false,
            input_path: input_path.to_string(),
            frame: ctx.load_texture(
                "video_frame",
                ColorImage::filled(
                    [init_size.width() as usize, init_size.height() as usize],
                    Color32::BLACK,
                ),
                Default::default(),
            ),
            frame_instant: Instant::now(),
            frame_pts: 0.0,
            frame_duration: 0.0,
            ctx: ctx.clone(),
            audio,
            subtitle: None,
            media_player,
            rx_metadata: streams.metadata,
            rx_video: streams.video,
            debug: false,
            avg_fps: 0.0,
            avg_fps_start: Instant::now(),
            frame_counter: 0,
            last_frame_counter: 0,
            error: None,
            osd: None,
            maintain_aspect: true,
            fullscreen: false,
            osd_end: Instant::now(),
            stream_info: None,
            rx_subtitle: streams.subtitle,
        })
    }

    /// Add an overlay for the player
    pub fn with_overlay(mut self, overlay: impl PlayerOverlay + 'static) -> Self {
        self.overlay = Box::new(overlay);
        self
    }

    #[allow(unused)]
    fn open_audio(
        state: SharedPlaybackState,
        rx: Receiver<AudioSamples>,
    ) -> Result<Box<dyn AudioDevice>> {
        #[cfg(feature = "audio")]
        return Ok(Box::new(
            crate::audio::AudioDevice::open_default_audio_stream(state, rx)?,
        ));
        Ok(Box::new(NoAudioDevice::new(rx)))
    }

    /// Render player in available space
    pub fn render(&mut self, ui: &mut Ui) -> Response {
        let size = ui.available_size();

        self.handle_keys(ui);
        self.process_state();
        let frame_response = self.render_frame(ui);
        self.render_subtitles(ui);
        self.render_overlay(ui, &frame_response);
        if let Some(error) = &self.error {
            ui.painter().text(
                pos2(size.x / 2.0, size.y / 2.0),
                Align2::CENTER_BOTTOM,
                error,
                FontId::proportional(30.),
                Color32::DARK_RED,
            );
        }
        if self.osd_end < Instant::now() {
            self.osd.take();
        }
        if let Some(osd) = &self.osd {
            ui.painter().text(
                pos2(size.x - 10.0, 50.0),
                Align2::RIGHT_TOP,
                osd,
                FontId::proportional(20.),
                Color32::WHITE,
            );
        }
        if self.debug {
            self.render_debug(ui, &frame_response);
        }
        frame_response
    }

    fn render_overlay(&mut self, ui: &mut Ui, frame: &Response) {
        self.overlay.show(ui, frame, &self.state);
    }
}

impl Widget for &mut Player {
    fn ui(self, ui: &mut Ui) -> Response {
        self.render(ui)
    }
}
