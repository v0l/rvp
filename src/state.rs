use crate::PlayerState;
use std::sync::Arc;
use std::sync::atomic::{
    AtomicBool, AtomicI8, AtomicI16, AtomicI64, AtomicIsize, AtomicU8, AtomicU16, AtomicU32,
    AtomicU64, Ordering,
};

/// Shared playback state
#[derive(Clone, Debug)]
pub struct SharedPlaybackState {
    volume: Arc<AtomicU8>,
    state: Arc<AtomicU8>,
    speed: Arc<AtomicU8>,
    mute: Arc<AtomicBool>,
    looping: Arc<AtomicBool>,
    duration: Arc<AtomicU64>,

    video_pts: Arc<AtomicI64>,
    audio_pts: Arc<AtomicI64>,
    subtitle_pts: Arc<AtomicI64>,

    // Current audio config
    pub sample_rate: Arc<AtomicU32>,
    pub channels: Arc<AtomicU8>,

    // current playback streams
    pub selected_video: Arc<AtomicIsize>,
    pub selected_audio: Arc<AtomicIsize>,
    pub selected_subtitle: Arc<AtomicIsize>,
}

impl SharedPlaybackState {
    /// PTS values (milliseconds)
    const PTS_SCALE: f64 = 1000.0;

    pub fn new() -> Self {
        Self {
            state: Arc::new(AtomicU8::new(PlayerState::Stopped as _)),
            volume: Arc::new(AtomicU8::new(u8::MAX)),
            speed: Arc::new(AtomicU8::new(20)),
            mute: Arc::new(AtomicBool::new(false)),
            looping: Arc::new(AtomicBool::new(false)),
            video_pts: Arc::new(AtomicI64::new(0)),
            audio_pts: Arc::new(AtomicI64::new(0)),
            subtitle_pts: Arc::new(AtomicI64::new(0)),
            duration: Arc::new(AtomicU64::new(0)),
            sample_rate: Arc::new(AtomicU32::new(48_000)),
            channels: Arc::new(AtomicU8::new(2)),
            selected_video: Arc::new(AtomicIsize::new(-1)),
            selected_audio: Arc::new(AtomicIsize::new(-1)),
            selected_subtitle: Arc::new(AtomicIsize::new(-1)),
        }
    }

    pub fn volume(&self) -> f32 {
        self.volume.load(Ordering::Relaxed) as f32 / u8::MAX as f32
    }

    fn scale_volume(volume: f32) -> u8 {
        (u8::MAX as f32 * volume.clamp(0.0, 1.0)) as _
    }

    pub fn set_volume(&self, volume: f32) {
        self.volume
            .store(Self::scale_volume(volume), Ordering::Relaxed);
    }

    pub fn incr_volume(&self, volume: f32) {
        let new_volume = self.volume() + volume;
        self.set_volume(new_volume);
    }

    pub fn decr_volume(&self, volume: f32) {
        let new_volume = self.volume() - volume;
        self.set_volume(new_volume);
    }

    pub fn state(&self) -> PlayerState {
        self.state.load(Ordering::Relaxed).into()
    }

    pub fn set_state(&self, new_state: PlayerState) {
        self.state.store(new_state as _, Ordering::Relaxed);
    }

    pub fn speed(&self) -> f32 {
        self.speed.load(Ordering::Relaxed) as f32 / 200.0 * 10.0
    }

    fn scale_speed(speed: f32) -> u8 {
        let f = speed.clamp(0.01, 10.0) / 10.0;
        (200.0 * f) as _
    }

    pub fn set_speed(&self, speed: f32) {
        self.speed
            .store(Self::scale_speed(speed), Ordering::Relaxed);
    }

    pub fn incr_speed(&self, speed: f32) {
        let new_speed = self.speed() + speed;
        self.set_speed(new_speed);
    }

    pub fn decr_speed(&self, speed: f32) {
        let new_speed = self.speed() - speed;
        self.set_speed(new_speed);
    }

    pub fn muted(&self) -> bool {
        self.mute.load(Ordering::Relaxed)
    }

    pub fn set_muted(&self, muted: bool) {
        self.mute.store(muted, Ordering::Relaxed);
    }

    pub fn looping(&self) -> bool {
        self.looping.load(Ordering::Relaxed)
    }

    pub fn set_looping(&self, looping: bool) {
        self.looping.store(looping, Ordering::Relaxed);
    }

    pub fn duration(&self) -> f64 {
        self.duration.load(Ordering::Relaxed) as f64 * Self::PTS_SCALE
    }

    pub fn set_duration(&self, new: f64) {
        self.duration
            .store((new * Self::PTS_SCALE) as _, Ordering::Relaxed);
    }

    pub fn video_pts(&self) -> f64 {
        self.video_pts.load(Ordering::Relaxed) as f64 / Self::PTS_SCALE
    }

    pub fn set_video_pts(&self, new: f64) {
        self.video_pts
            .store((new * Self::PTS_SCALE) as _, Ordering::Relaxed);
    }

    pub fn audio_pts(&self) -> f64 {
        self.audio_pts.load(Ordering::Relaxed) as f64 / Self::PTS_SCALE
    }

    pub fn set_audio_pts(&self, new: f64) {
        self.audio_pts
            .store((new * Self::PTS_SCALE) as _, Ordering::Relaxed);
    }

    pub fn incr_audio_pts(&self, new: f64) {
        self.audio_pts
            .fetch_add((new * Self::PTS_SCALE) as _, Ordering::Relaxed);
    }

    pub fn subtitle_pts(&self) -> f64 {
        self.subtitle_pts.load(Ordering::Relaxed) as f64 / Self::PTS_SCALE
    }

    pub fn set_subtitle_pts(&self, new: f64) {
        self.subtitle_pts
            .store((new / Self::PTS_SCALE) as _, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume() {
        let state = SharedPlaybackState::new();
        state.set_volume(0.8);
        assert_eq!(state.volume(), 0.8);
        state.incr_volume(0.1);
        assert_eq!((state.volume() * 10.0).round(), 9.0);
        state.decr_volume(0.1);
        assert_eq!((state.volume() * 10.0).round(), 8.0);
        state.set_volume(11.8);
        assert_eq!(state.volume(), 1.0);
        state.set_volume(-11.8);
        assert_eq!(state.volume(), 0.0);
    }

    #[test]
    fn speed() {
        let state = SharedPlaybackState::new();
        state.set_speed(1.0);
        assert_eq!(state.speed(), 1.0);
        state.incr_speed(0.1);
        assert_eq!((state.speed() * 10.0).round(), 11.0);
        state.incr_speed(0.1);
        assert_eq!((state.speed() * 10.0).round(), 12.0);
        state.decr_speed(0.1);
        assert_eq!((state.speed() * 10.0).round(), 11.0);
        state.set_speed(99.0);
        assert_eq!((state.speed() * 10.0).round(), 100.0);
    }
}
