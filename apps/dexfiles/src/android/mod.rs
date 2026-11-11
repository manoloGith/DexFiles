//! The Android side: the entry point, and DexFiles' own bridge to its `MainActivity` (Kotlin)
//! for what a file manager needs beyond craft-android: storage volumes, the "All files access"
//! permission, opening files with other apps, sharing, telling the media index about changes.
//!
//! Everything else (the JNI bridge to CraftActivity, the environment, mouse, keyboard,
//! lifecycle, clipboard, "Open with" into the app, diagnostics) is craft-android's.

use std::path::{Path, PathBuf};
use std::sync::{PoisonError, RwLock};

use craft_android::android::{self as platform, AndroidApp};
use jni::objects::{Global, JObject, JString};
use jni::{EnvUnowned, JValue, JavaVM, jni_sig, jni_str};
use serde_json::json;

/// Entry point, called by android-activity on the activity's own thread.
#[allow(unsafe_code)] // `no_mangle` is how android-activity's glue finds the entry point
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    let config = platform::Config {
        log_tag: "dexfiles",
        asset_dirs: Vec::new(),
    };
    let Some(started) = platform::start(&app, &config) else {
        return;
    };
    platform::diag(&format!("DexFiles {}", env!("CARGO_PKG_VERSION")));
    let data = started.data_dir;
    crate::platform::set_data_dir(data.clone());
    let launch = crate::Launch {
        open: Vec::new(),
        persistence_path: Some(data.join("config/app.ron")),
    };
    let options_launch = launch.clone();
    crate::run(
        move |renderer| {
            platform::native_options(&app, crate::native_options(renderer, &options_launch))
        },
        launch,
    );
    platform::finish();
}

/// The `MainActivity` (replaced when Android recreates it).
static ACTIVITY: RwLock<Option<Global<JObject<'static>>>> = RwLock::new(None);

/// `MainActivity.files(op, json)`: its JSON answer ("{}" when there is no activity or it failed).
fn call(op: &str, args: serde_json::Value) -> serde_json::Value {
    let guard = ACTIVITY.read().unwrap_or_else(PoisonError::into_inner);
    let Some(activity) = guard.as_ref() else {
        return json!({});
    };
    let vm = match JavaVM::singleton() {
        Ok(vm) => vm,
        Err(e) => {
            log::warn!("dexfiles: no Java VM for {op}: {e}");
            return json!({});
        }
    };
    let text = args.to_string();
    let result = vm.attach_current_thread(|env| -> jni::errors::Result<String> {
        let op_j = env.new_string(op)?;
        let json_j = env.new_string(&text)?;
        let out = env
            .call_method(
                activity.as_obj(),
                jni_str!("files"),
                jni_sig!("(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;"),
                &[JValue::Object(&op_j), JValue::Object(&json_j)],
            )?
            .l()?;
        if out.is_null() {
            return Ok("{}".to_owned());
        }
        let s = env.cast_local::<JString>(out)?;
        s.try_to_string(env)
    });
    match result {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| json!({})),
        Err(e) => {
            log::warn!("dexfiles: files {op} failed: {e}");
            json!({})
        }
    }
}

/// `MainActivity.nativeRegisterFiles(activity)`.
#[allow(unsafe_code)] // `no_mangle`: the JVM finds native methods by their exported name
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_dexfiles_MainActivity_nativeRegisterFiles<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _this: JObject<'caller>,
    activity: JObject<'caller>,
) {
    unowned
        .with_env(|env| -> jni::errors::Result<()> {
            let global = env.new_global_ref(&activity)?;
            *ACTIVITY.write().unwrap_or_else(PoisonError::into_inner) = Some(global);
            log::info!("dexfiles: activity registered");
            Ok(())
        })
        .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

pub fn storage_access() -> bool {
    call("storage", json!({}))["granted"]
        .as_bool()
        .unwrap_or(false)
}

pub fn request_storage_access() {
    call("request_storage", json!({}));
}

pub fn volumes() -> Vec<crate::platform::Volume> {
    let answer = call("volumes", json!({}));
    let mut out = Vec::new();
    for v in answer["volumes"].as_array().into_iter().flatten() {
        let Some(path) = v["path"].as_str().filter(|p| !p.is_empty()) else {
            continue;
        };
        out.push(crate::platform::Volume {
            path: PathBuf::from(path),
            label: v["label"].as_str().unwrap_or_default().to_owned(),
            removable: v["removable"].as_bool().unwrap_or(false),
        });
    }
    out
}

pub fn open_file(path: &Path, chooser: bool) -> Result<(), String> {
    let answer = call(
        "open",
        json!({ "path": path.to_string_lossy(), "chooser": chooser }),
    );
    match answer["error"].as_str() {
        Some(e) => Err(e.to_owned()),
        None => Ok(()),
    }
}

pub fn share(paths: &[PathBuf], email: bool) -> Result<(), String> {
    let list: Vec<String> = paths
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let answer = call("share", json!({ "paths": list, "email": email }));
    match answer["error"].as_str() {
        Some(e) => Err(e.to_owned()),
        None => Ok(()),
    }
}

pub fn media_scan(paths: &[PathBuf]) {
    if paths.is_empty() {
        return;
    }
    let list: Vec<String> = paths
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    call("media_scan", json!({ "paths": list }));
}

/// The apps that handle a type, with their icons (PNG, base64).
pub fn apps_for(ext: &str, mime: &str) -> Vec<crate::apps::AppInfo> {
    use base64::Engine as _;
    let answer = call("apps_for", json!({ "ext": ext, "mime": mime }));
    let mut out = Vec::new();
    for a in answer["apps"].as_array().into_iter().flatten() {
        let Some(id) = a["id"].as_str() else { continue };
        let icon = a["icon"]
            .as_str()
            .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok())
            .and_then(|png| crate::apps::icon_from_png(&png));
        out.push(crate::apps::AppInfo {
            id: id.to_owned(),
            label: a["label"].as_str().unwrap_or(id).to_owned(),
            icon,
        });
    }
    out
}

/// Open a file with one app (`package/activity`).
pub fn open_with(path: &Path, app: &str) -> Result<(), String> {
    let answer = call(
        "open",
        json!({ "path": path.to_string_lossy(), "app": app }),
    );
    match answer["error"].as_str() {
        Some(e) => Err(e.to_owned()),
        None => Ok(()),
    }
}

/// A video's frame (scaled to `max` pixels) and details, decoded by Android's media framework.
pub fn video_frame(path: &Path, max: u32) -> Option<crate::preview::MediaInfo> {
    use base64::Engine as _;
    let answer = call(
        "video_frame",
        json!({ "path": path.to_string_lossy(), "max": max }),
    );
    if answer.get("error").is_some() || answer.as_object().is_none_or(|o| o.is_empty()) {
        return None;
    }
    let image = answer["frame"]
        .as_str()
        .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok())
        .and_then(|png| crate::apps::icon_from_png(&png));
    let mut info = Vec::new();
    if let Some(ms) = answer["duration_ms"].as_u64() {
        info.push(("media_duration", crate::preview::format_duration(ms / 1000)));
    }
    if let (Some(w), Some(h)) = (answer["width"].as_u64(), answer["height"].as_u64()) {
        info.push(("media_resolution", format!("{w} × {h}")));
    }
    for (key, field) in [("media_title", "title"), ("media_artist", "artist")] {
        if let Some(v) = answer[field].as_str().filter(|v| !v.is_empty()) {
            info.push((key, v.to_owned()));
        }
    }
    Some((image, info))
}

/// The media player in the activity (`player_<op>`); its JSON answer.
pub fn player(op: &str, args: serde_json::Value) -> serde_json::Value {
    call(&format!("player_{op}"), args)
}

fn strings(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

/// Copied local files on the system clipboard (as content URIs), for other apps to paste.
pub fn clip_set(paths: &[PathBuf]) {
    call("clip_set", json!({ "paths": strings(paths) }));
}

/// What a paste takes from the system clipboard: `None` when it holds DexFiles' own copy (or no
/// files), otherwise the files another app put there, copied into the cache.
pub fn clip_import() -> Option<Vec<PathBuf>> {
    let answer = call("clip_import", json!({}));
    if answer["ours"].as_bool() == Some(true) {
        return None;
    }
    let paths: Vec<PathBuf> = answer["paths"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| p.as_str().map(PathBuf::from))
        .collect();
    (!paths.is_empty()).then_some(paths)
}

/// Hand a drag of local files to the system, for other apps to take.
pub fn drag_out(paths: &[PathBuf]) {
    call("drag_out", json!({ "paths": strings(paths) }));
}
