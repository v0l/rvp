# rvp, a video playing library for [`egui`](https://github.com/emilk/egui)
[![crates.io](https://img.shields.io/crates/v/egui-video)](https://crates.io/crates/egui-video)
[![docs](https://docs.rs/egui-video/badge.svg)](https://docs.rs/egui-video/latest/egui_video/)
[![license](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/v0l/rvp/blob/main/LICENSE)

Plays videos in egui using various supported decoder backends.

## Dependencies:
 - requires ffmpeg 6 or 7. follow the build instructions [here](https://github.com/zmwangx/rust-ffmpeg/wiki/Notes-on-building)

## Usage:
```rust
use rvp::{Player, DefaultOverlay}
// called once (creating a player)
let mut player = Player::new(ctx, my_media_path)?.with_overlay(DefaultOverlay);
// called every frame (showing the player)
player.ui(ui, player.size);
```