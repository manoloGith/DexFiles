# Fase 5: lo de DexFiles que pasa a craft-android

DexFiles resolvió por su cuenta, con su propio puente JNI (`MainActivity.files` y
`src/android/mod.rs`), varias cosas que otras apps Dex también necesitan: abrir con otra app,
compartir, portapapeles de archivos, arrastrar hacia otras apps… Este documento dice qué pasa a
craft-android, con qué forma, y cómo se adapta DexFiles después.

El trabajo en craft-android se hace en su propio hilo, con el texto del final. Base de partida:
craft-android `6a93b5f`, el commit que usa DexFiles.

## Qué pasa y qué se queda

| En DexFiles hoy | ¿A craft-android? | Por qué |
|---|---|---|
| `open`: abrir un archivo con la app del sistema o con el selector | **Sí** | Cualquier app puede querer abrir un adjunto o una exportación |
| `open` con `app` y `apps_for` (lista de apps con iconos) | **Sí** | Es la base de «Abrir con…» |
| `share` / `email` | **Sí** | Compartir y enviar por correo |
| `clip_set` (archivos como URI) / `clip_import` (archivos copiados por otra app) | **Sí**, ampliando `clipboard::Clip::Files` | craft-android ya tiene `Clip::Files`, pero solo escribe y solo para el almacenamiento propio de la app |
| `drag_out`: arrastre del sistema con archivos | **Sí** | Arrastrar un PDF exportado o una imagen a otra app en DeX |
| `media_scan` | **Sí** | Cualquier app que guarde fotos, audio o vídeo en el almacenamiento compartido |
| `dex_apps` (apps Dex y sus extensiones) | **Sí**, la lectura | Cualquier app puede ofrecer «abrir con una app Dex». La declaración ya es solo manifiesto (`docs/apps-dex.md`) |
| `volumes`, `storage` / `request_storage` («Acceso a todos los archivos») | **Sí, opcional** | Solo para apps que gestionan archivos; craft-android no debe pedir ese permiso por defecto |
| `video_frame` | **Sí, opcional** | Miniaturas de vídeo para cualquier app con vista previa |
| `player_*` (reproductor con SurfaceView) | No | Es propio de la interfaz de DexFiles |

## Decisiones de diseño

1. **Un solo puente.** Todo pasa por `CraftActivity.bridge(op, json, data)` y
   `craft_android::android::bridge::call`, como el resto de craft-android. Al terminar, DexFiles
   no necesita `nativeRegisterFiles` ni `MainActivity.files`, salvo para el reproductor.

2. **Qué archivos se pueden entregar a otras apps.** El FileProvider de craft-android
   (`${applicationId}.files`) solo cubre `cache/clipboard`, `files/documents` y `files/folders`.
   Un explorador necesita entregar cualquier ruta. Propuesta:
   - craft-android pasa a tener un único método `CraftActivity.uriFor(file): Uri`, que usan abrir,
     compartir, portapapeles y arrastre;
   - por defecto usa el FileProvider de craft-android, con sus rutas actuales;
   - una app puede ampliarlo con `override fun shareablePaths()` o con su propio recurso de rutas.
     DexFiles lo amplía a `root-path` (todo), como hace hoy su `DexFilesProvider`;
   - los archivos fuera de esas rutas se copian antes a `cache/share/` y se entrega la copia.
     Así una app sin `root-path` también puede compartir lo que esté fuera de su almacenamiento.

3. **El portapapeles de archivos en las dos direcciones.**
   - `Clip::Files(paths)` al escribir: cada elemento lleva la URI y la ruta como texto, y la
     etiqueta del ClipData identifica a la app, para reconocer luego su propia copia.
   - `clipboard::get()` devuelve también `Clip::Files` cuando otra app copió archivos. Los copia a
     `cache/clipboard-import/<marca de tiempo>/` con su nombre (`OpenableColumns.DISPLAY_NAME`).
     Copiar puede tardar, así que debe llamarse fuera del hilo de la interfaz, como ya indica la
     documentación de `get`.
   - Una función `clipboard::is_own()`, o un campo `own: bool` en la respuesta, distingue la copia
     de la propia app (la app pega desde su portapapeles interno) de los archivos de otra.

4. **Arrastrar hacia fuera.**
   - `drag::start(paths, label)` lanza `View.startDragAndDrop` con `DRAG_FLAG_GLOBAL |
     DRAG_FLAG_GLOBAL_URI_READ` y una sombra con el nombre o el número de archivos. Es la clase
     `LabelShadow` de DexFiles.
   - `drag::leaving(ctx) -> bool` dice si el puntero, con un botón pulsado, llegó al borde de la
     ventana o salió de ella (`PointerGone`). Cada app decide qué arrastra: craft-android no conoce
     los *payloads* de egui de cada una.
   - **Soltar en la propia ventana.** `acceptDrops()` de CraftActivity recibiría su propio arrastre
     y copiaría los archivos. Con `localState` marcado (`DRAG_LOCAL`), el evento `open` debe llevar
     `own: true` y las rutas originales en lugar de copias. DexFiles hoy lo resuelve comparando
     nombres, que es frágil.

5. **Apps Dex.**
   - `apps::dex_apps(key) -> Vec<DexApp>` consulta `io.github.dexfiles.action.OPEN` con
     `GET_META_DATA`.
   - El `<queries>` con esa acción va en el manifiesto de craft-activity, para que lo tengan todas
     las apps.
   - La política (cuándo asignar una extensión, no volver a asignar la que se quitó) se queda en
     cada app: es configuración suya.

6. **Permiso de almacenamiento, opcional.**
   - `storage::all_files_access() -> bool`, `storage::request_all_files_access()` y
     `storage::volumes() -> Vec<Volume { path, label, removable }>`.
   - El permiso `MANAGE_EXTERNAL_STORAGE` **no** va en el manifiesto de craft-activity: lo declara
     la app que lo necesite. Sin él, `request_all_files_access` no hace nada y registra un aviso.

## API propuesta (Rust, `craft_android::android`)

```rust
pub mod apps {
    pub struct AppInfo { pub id: String, pub label: String, pub icon_png: Option<Vec<u8>> }
    pub struct DexApp { pub id: String, pub label: String, pub extensions: Vec<String> }
    /// Abrir con la app predeterminada del sistema, o con su selector.
    pub fn open(path: &Path, chooser: bool) -> Result<(), String>;
    /// Abrir con una app concreta (`paquete/actividad`).
    pub fn open_with(path: &Path, app: &str) -> Result<(), String>;
    /// Las apps que abren ese tipo de archivo, con su icono en PNG (bloquea: fuera del hilo de UI).
    pub fn apps_for(ext: &str, mime: &str) -> Vec<AppInfo>;
    /// Las apps Dex que declaran extensiones (`key`: el nombre del meta-data).
    pub fn dex_apps(key: &str) -> Vec<DexApp>;
}
pub mod share {
    /// La hoja de compartir (`email`: solo apps de correo, como adjuntos).
    pub fn share(paths: &[PathBuf], email: bool) -> Result<(), String>;
}
pub mod clipboard {
    // Clip::Files ya existe; get() lo devuelve también para archivos de otras apps.
    pub fn is_own() -> bool;
}
pub mod drag {
    pub fn start(paths: &[PathBuf], label: &str);
    pub fn leaving(ctx: &egui::Context) -> bool;
}
pub mod media {
    pub fn scan(paths: &[PathBuf]);
    pub struct VideoFrame { pub png: Option<Vec<u8>>, pub width: u32, pub height: u32, pub duration_ms: u64, pub title: Option<String>, pub artist: Option<String> }
    pub fn video_frame(path: &Path, max: u32) -> Option<VideoFrame>;
}
pub mod storage {
    pub struct Volume { pub path: PathBuf, pub label: String, pub removable: bool }
    pub fn all_files_access() -> bool;
    pub fn request_all_files_access();
    pub fn volumes() -> Vec<Volume>;
}
```

Operaciones del puente (Kotlin, en `CraftActivity.bridge`): `open`, `apps_for`, `dex_apps`,
`share`, `clip_files` (ya existe; se amplía), `clip_get` (devuelve `files` y `own`), `drag_out`,
`media_scan`, `video_frame`, `storage`, `request_storage` y `volumes`. El código de partida es
el de DexFiles (`MainActivity.kt`: `open`, `appsFor`, `iconPng`, `share`, `clipSet`,
`clipImport`, `displayName`, `dragOut`, `LabelShadow`, `dexApps`, `mediaScan`, `videoFrame`,
`volumes`, `volumePath`, `hasStorageAccess` y `requestStorageAccess`), con licencia MIT o
Apache-2.0, como craft-android.

En **escritorio**, las mismas funciones existen en DexFiles (`apps.rs`, `platform.rs`): `.desktop`,
`xdg-open` y `xdg-email`. Pueden pasar también a craft-android, detrás de
`cfg(not(target_os = "android"))`, si otras apps las quieren. No es necesario para la fase.

## Después: DexFiles

Cuando craft-android publique la versión con esto:

1. Actualizar el submódulo y el `rev`.
2. `src/android/mod.rs` se reduce a `android_main` y al reproductor; `platform.rs` y `apps.rs`
   llaman a `craft_android::android::{apps, share, clipboard, drag, media, storage}`.
3. `MainActivity.kt` se queda con el reproductor y `shareablePaths()` (`root-path`);
   desaparecen `DexFilesProvider` y `res/xml/dexfiles_paths.xml` si el FileProvider de
   craft-android admite las rutas de la app.
4. Arrastrar hacia fuera usa `drag::leaving` y `drag::start`. Soltar en la propia ventana usa el
   `own: true` del evento `open`, y desaparece la comparación de nombres.
5. Pegar usa `clipboard::get()` en un hilo: `Clip::Files` de otra app se mueve a la carpeta, y si
   es la copia propia se pega desde el portapapeles interno.

## Texto para el hilo de craft-android

> Quiero pasar a craft-android lo que DexFiles hizo por su cuenta y sirve a todas las apps Dex. El
> diseño está en `docs/fase5-craft-android.md` del repo DexFiles (rama
> `claude/serene-johnson-whj8jb`), y el código de partida en
> `apps/dexfiles/android/app/src/main/java/io/github/dexfiles/MainActivity.kt` y
> `apps/dexfiles/src/android/mod.rs` de ese repo.
>
> Implementa en craft-android:
>
> 1. Los módulos `apps`, `share`, `drag`, `media` y `storage`, y la ampliación de `clipboard`,
>    con la API de ese documento. Todo por `CraftActivity.bridge`.
> 2. Un único `CraftActivity.uriFor(file)` para abrir, compartir, portapapeles y arrastre:
>    - usa el FileProvider de craft-android;
>    - una app puede ampliar sus rutas (`shareablePaths()` o su propio recurso);
>    - lo que quede fuera se copia antes a `cache/share/`.
> 3. `clipboard::get()`:
>    - devuelve `Clip::Files` cuando otra app copió archivos (copiados a la caché con su nombre);
>    - `clipboard::is_own()` distingue la copia de la propia app.
> 4. `drag::start(paths, label)`, con una sombra que muestre el nombre o el número de archivos.
>    `drag::leaving(ctx)` dice si el arrastre llegó al borde de la ventana. El evento `open` lleva
>    `own: true` y las rutas originales si se suelta en la propia ventana un arrastre de la app.
> 5. `apps::dex_apps(key)`, con `<queries>` para `io.github.dexfiles.action.OPEN` en el manifiesto
>    de craft-activity.
> 6. `storage` es opcional: el permiso `MANAGE_EXTERNAL_STORAGE` lo declara solo la app que lo
>    necesite, no craft-activity.
>
> Documenta cada función, añade tests de lo que se pueda probar sin dispositivo (el protocolo
> JSON) y publica una versión nueva. Después adaptaré DexFiles para usarla.
