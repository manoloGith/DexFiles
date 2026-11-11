# DexFiles

Un explorador de archivos moderno, al estilo de Finder, para **Samsung DeX** y el modo escritorio
de Android (monitor, ratón y teclado), que también funciona en el móvil y en Linux.

Está hecho en Rust con egui, igual que [DexPdfCraft](https://github.com/manoloGith/DexPdfCraft), y
usa la misma capa Android compartida, [craft-android](https://github.com/manoloGith/craft-android):
ratón completo (rueda, botón derecho, botones atrás/adelante), teclado físico, teclado en pantalla,
cierre y reapertura fiables, y un informe de diagnóstico que se copia sin adb.

- **Navegación**: barra lateral con favoritos, dispositivos en árbol, red y papelera; barra de
  direcciones editable; atrás/adelante/subir; pestañas; **dos paneles**; vista previa a la derecha.
- **Red**: FTP/FTPS, SFTP, SMB 2/3 y WebDAV, en Rust puro.
- **Operaciones**: copiar, cortar, pegar, renombrar, papelera o borrado permanente, nuevas
  carpetas, en segundo plano con progreso; arrastrar y soltar; ZIP y 7z (con contraseña), TAR(.gz/.bz2/.xz/.zst) y RAR (APK de GitHub);
  compartir y enviar por correo.
- **Búsqueda**: filtro instantáneo y búsqueda en subcarpetas por nombre, contenido, tipo, fecha y
  tamaño.
- **Vistas**: iconos grandes/medianos/pequeños, lista, detalles y contenido; orden por nombre,
  fecha, tipo, tamaño o estado; ocultos y extensiones; carpeta de inicio configurable.

Estado y plan por fases: [docs/plan.md](docs/plan.md). Compresión (librerías y propuesta para RAR):
[docs/compression.md](docs/compression.md). Cómo probarlo: [docs/testing.md](docs/testing.md).

## Estructura

```
DexFiles/
├── craft-android/            submódulo: la capa Android compartida (y winit con los parches de DeX)
├── apps/dexfiles/            la app (cdylib para Android + binario de escritorio)
│   ├── src/vfs/              sistemas de archivos: local, FTP, SFTP, SMB, WebDAV
│   ├── src/ops.rs            operaciones en segundo plano (copiar, mover, borrar…)
│   ├── src/trash.rs          papelera (especificación freedesktop)
│   ├── src/archive.rs        comprimir y extraer
│   ├── src/search.rs         búsqueda
│   ├── src/preview.rs        vistas previas y miniaturas
│   ├── src/ui/               la interfaz egui
│   ├── src/android/          android_main y el puente con MainActivity
│   ├── strings.tsv           textos en inglés y español
│   └── android/              proyecto Gradle (MainActivity : CraftActivity)
└── docs/
```

## Compilar

```bash
git clone --recursive https://github.com/manoloGith/DexFiles
```

Escritorio (para iterar sin dispositivo):

```bash
cargo run -p dexfiles --features desktop
```

APK (Rust según `rust-toolchain.toml`, `cargo-ndk` 4.1.2, NDK 29.0.14206865, JDK 17, SDK 36):

```bash
cd apps/dexfiles/android && ./gradlew assembleDebug -Pdexfiles.abis=arm64-v8a
```

## Licencia

MIT o Apache-2.0, a elegir.
