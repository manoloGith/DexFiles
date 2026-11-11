//! Playing audio and video. On Android the system's `MediaPlayer` plays (hardware decoding),
//! and a video is shown in a surface of the activity placed over the player's area of the
//! window (the interface tells where, [`set_video_rect`]). Elsewhere there is no engine: the
//! player says so, unless `DEXFILES_SIMULATED_PLAYER` is set, which fakes the clock (to work
//! on the interface without a device; it makes no sound).

use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub position_ms: u64,
    pub duration_ms: u64,
    pub playing: bool,
    /// The file has started (it may be paused).
    pub ready: bool,
    pub ended: bool,
    /// A video's size in pixels.
    pub video: Option<(u32, u32)>,
    pub error: Option<String>,
}

/// Whether this build can play anything.
pub fn available() -> bool {
    cfg!(target_os = "android") || sim::enabled()
}

pub fn open(path: &Path, video: bool) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        crate::android::player(
            "open",
            serde_json::json!({ "path": path.to_string_lossy(), "video": video }),
        );
        Ok(())
    }
    #[cfg(not(target_os = "android"))]
    {
        sim::open(path, video)
    }
}

pub fn play() {
    command("play", serde_json::json!({}));
}

pub fn pause() {
    command("pause", serde_json::json!({}));
}

pub fn seek(ms: u64) {
    command("seek", serde_json::json!({ "ms": ms }));
}

pub fn stop() {
    command("stop", serde_json::json!({}));
}

/// Where the video goes, in the window's pixels (`None`: hide it, the sound goes on).
pub fn set_video_rect(rect: Option<[i32; 4]>) {
    let args = match rect {
        Some([x, y, w, h]) => {
            serde_json::json!({ "x": x, "y": y, "w": w, "h": h, "visible": true })
        }
        None => serde_json::json!({ "visible": false }),
    };
    command("rect", args);
}

pub fn state() -> State {
    #[cfg(target_os = "android")]
    {
        let v = crate::android::player("state", serde_json::json!({}));
        let size = match (v["width"].as_u64(), v["height"].as_u64()) {
            (Some(w), Some(h)) if w > 0 && h > 0 => Some((w as u32, h as u32)),
            _ => None,
        };
        State {
            position_ms: v["position"].as_u64().unwrap_or(0),
            duration_ms: v["duration"].as_u64().unwrap_or(0),
            playing: v["playing"].as_bool().unwrap_or(false),
            ready: v["ready"].as_bool().unwrap_or(false),
            ended: v["ended"].as_bool().unwrap_or(false),
            video: size,
            error: v["error"].as_str().map(str::to_owned),
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        sim::state()
    }
}

fn command(op: &str, args: serde_json::Value) {
    #[cfg(target_os = "android")]
    {
        crate::android::player(op, args);
    }
    #[cfg(not(target_os = "android"))]
    sim::command(op, &args);
}

#[cfg_attr(target_os = "android", allow(dead_code))]
mod sim {
    //! A stand-in engine for desktop development: a clock, no sound.

    use std::sync::{Mutex, PoisonError};
    use std::time::Instant;

    use super::{Path, State};

    struct Sim {
        duration_ms: u64,
        /// Position when last paused, and since when it plays.
        base_ms: u64,
        since: Option<Instant>,
        video: bool,
    }

    static SIM: Mutex<Option<Sim>> = Mutex::new(None);

    pub fn enabled() -> bool {
        std::env::var_os("DEXFILES_SIMULATED_PLAYER").is_some()
    }

    pub fn open(path: &Path, video: bool) -> Result<(), String> {
        if !enabled() {
            return Err("no media engine on this system".into());
        }
        let duration_ms = crate::preview::audio(path, 16)
            .and_then(|(_, info)| {
                info.into_iter()
                    .find(|(k, _)| *k == "media_duration")
                    .map(|(_, v)| v)
            })
            .map(|v| {
                let parts: Vec<u64> = v.split(':').filter_map(|p| p.parse().ok()).collect();
                parts.iter().fold(0, |acc, p| acc * 60 + p) * 1000
            })
            .filter(|d| *d > 0)
            .unwrap_or(180_000);
        *SIM.lock().unwrap_or_else(PoisonError::into_inner) = Some(Sim {
            duration_ms,
            base_ms: 0,
            since: Some(Instant::now()),
            video,
        });
        Ok(())
    }

    fn position(s: &Sim) -> u64 {
        let run = s.since.map(|t| t.elapsed().as_millis() as u64).unwrap_or(0);
        (s.base_ms + run).min(s.duration_ms)
    }

    pub fn command(op: &str, args: &serde_json::Value) {
        let mut guard = SIM.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(s) = guard.as_mut() else { return };
        match op {
            "play" if s.since.is_none() => s.since = Some(Instant::now()),
            "pause" => {
                s.base_ms = position(s);
                s.since = None;
            }
            "seek" => {
                s.base_ms = args["ms"].as_u64().unwrap_or(0).min(s.duration_ms);
                if s.since.is_some() {
                    s.since = Some(Instant::now());
                }
            }
            "stop" => *guard = None,
            _ => {}
        }
    }

    pub fn state() -> State {
        let guard = SIM.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(s) = guard.as_ref() else {
            return State::default();
        };
        let pos = position(s);
        State {
            position_ms: pos,
            duration_ms: s.duration_ms,
            playing: s.since.is_some() && pos < s.duration_ms,
            ready: true,
            ended: pos >= s.duration_ms,
            video: s.video.then_some((1280, 720)),
            error: None,
        }
    }
}
