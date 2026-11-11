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
