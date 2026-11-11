# Compresión: qué librería integrar

Pediste estudiar qué librería usar para comprimir y descomprimir (.zip, .7z, .rar…). Estas son
las opciones que valoré y lo que he integrado.

## Criterios

1. **Que compile para Android sin dolor** (cargo-ndk, arm64 y x86_64). Rust puro es lo ideal; el C
   pequeño (como `ring`) es aceptable; C++ con CMake complica el build y la CI.
2. **Licencia libre** compatible con MIT/Apache y con F-Droid.
3. **Streaming y progreso**: poder informar del avance y cancelar.
4. **Tamaño** del APK.

## Opciones

| Opción | Formatos | Lenguaje | Licencia | Comentario |
|---|---|---|---|---|
| **`zip`** (zip-rs) | ZIP (leer/escribir, deflate, zip64, AES) | Rust | MIT | El estándar en Rust. Integrado. |
| **`sevenz-rust2`** | 7z (leer/escribir: LZMA, LZMA2, BCJ, PPMd, bzip2…; AES opcional) | Rust | Apache-2.0 | Fork mantenido de `sevenz-rust`. Integrado. |
| **`tar`** + **`flate2`** / **`bzip2`** (`libbz2-rs-sys`) / **`lzma-rust2`** | TAR, .tar.gz, .tar.bz2, .tar.xz y .gz/.bz2/.xz sueltos | Rust | MIT/Apache | Integrados. `lzma-rust2` es la misma base que usa `sevenz-rust2` (sin duplicar código). |
| `zstd` / `ruzstd` | .zst, .tar.zst | C / Rust | BSD / MIT | Fácil de añadir; `ruzstd` solo descomprime. Propuesto para la fase 2. |
| `unrar` (crate) | RAR (solo extraer) | C++ (UnRAR de RARLAB) | **UnRAR license (no libre)** | Compila con el NDK, pero la licencia no es OSI y **F-Droid no lo acepta**. |
| `compress-tools` (libarchive) | Casi todo (incl. RAR5 de lectura, ISO, CPIO) | C | BSD | Muy completo, pero hay que compilar libarchive + sus dependencias para cada ABI; build frágil. |
| Llamar a `7z`/`p7zip` como programa | Todo lo de 7-Zip, incluido RAR | C++ | LGPL + unRAR | Como Tesseract en DexPdfCraft (`lib<abi>/lib7z.so` + `nativePrograms`). Pesado (~3 MB) y RAR sigue con la licencia unRAR. |

## Lo integrado (fase 1)

- **Comprimir**: ZIP, 7z, TAR, TAR.GZ, TAR.BZ2, TAR.XZ, desde el menú contextual
  (*Comprimir ▸*) o con el diálogo de opciones (nombre y formato).
- **Extraer**: ZIP (y JAR, APK, CBZ, EPUB, que son ZIP), 7z, TAR(.gz/.bz2/.xz), .gz/.bz2/.xz
  sueltos. *Extraer aquí* o *Extraer en «carpeta»*. Los nombres que intentan salir de la carpeta
  (`../`, rutas absolutas) se rechazan.
- **Vista previa**: la lista de contenidos de cualquiera de ellos en el panel derecho.
- Progreso y cancelación como cualquier otra operación. Todo en Rust puro: no añade nada al build
  de Android.

## RAR: integrado como *feature* `rar` (fase 2)

Elegiste la opción 1. La *feature* `rar` añade la crate `unrar` (UnRAR de RARLAB, C++): listar y
extraer, también con contraseña. Está **desactivada por defecto**; la CI de GitHub y los APK que
se publiquen allí la activan (`./gradlew assembleDebug -Pdexfiles.features=rar`), y un build para
F-Droid simplemente no la pasa, así que no lleva código de UnRAR. Detalles del build en Android:
C++ enlazado estáticamente (no hace falta `libc++_shared.so`) y un `libpthread.a` vacío en
`build.rs`, porque `unrar-sys` pide `-lpthread` y en Android los hilos están en libc.

También en la fase 2: **Zstandard** (`.zst`, `.tar.zst`, con `ruzstd`, Rust puro) y
**contraseñas**: crear ZIP y 7z cifrados con AES-256 (campo *Contraseña* del diálogo
*Comprimir*) y, al extraer un ZIP, 7z o RAR cifrado, DexFiles pide la contraseña (y la vuelve a
pedir si era incorrecta).

## La propuesta original para RAR

1. **Recomendado**: dejar RAR fuera del APK de F-Droid y ofrecer una *feature* `rar` (crate
   `unrar`) para el APK de GitHub. Solo extracción (el formato no permite crear RAR legalmente).
2. Alternativa sin licencias raras: **libarchive** lee RAR (v4 y parcialmente v5) con licencia
   BSD; a cambio, el build de Android se complica (CMake + zlib/bzip2/lzma/zstd por ABI).
3. Mientras tanto, *Abrir con…* manda el .rar a otra app (RAR de RARLAB, ZArchiver…).

