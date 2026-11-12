# Distribución: GitHub, Obtainium y F-Droid

## GitHub y Obtainium

Cada versión se publica como release de GitHub con el APK firmado (ver
[releasing.md](releasing.md)). [Obtainium](https://github.com/ImranR98/Obtainium) puede seguir
`https://github.com/manoloGith/DexFiles` e instalar las actualizaciones según salen.

## F-Droid (preparado, sin enviar)

El repositorio tiene los metadatos en el formato que lee F-Droid, en
`fastlane/metadata/android/{en-US,es-ES}`: título, descripciones, notas por `versionCode`, icono y
capturas. Para pedir la inclusión hay que abrir una *merge request* en
[fdroiddata](https://gitlab.com/fdroid/fdroiddata) con `metadata/io.github.dexfiles.yml`. Este es
el borrador:

```yaml
Categories:
  - System
License: MIT OR Apache-2.0
SourceCode: https://github.com/manoloGith/DexFiles
IssueTracker: https://github.com/manoloGith/DexFiles/issues
Changelog: https://github.com/manoloGith/DexFiles/releases

AutoName: DexFiles

RepoType: git
Repo: https://github.com/manoloGith/DexFiles

Builds:
  - versionName: 0.1.0
    versionCode: 1
    commit: v0.1.0
    subdir: apps/dexfiles/android/app
    submodules: true
    gradle:
      - yes
    output: build/outputs/apk/release/app-release-unsigned.apk
    prebuild:
      - curl -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain none
      - source $HOME/.cargo/env
      - rustup toolchain install $(sed -n 's/^channel = "\(.*\)"/\1/p' ../../../../rust-toolchain.toml)
      - cargo install cargo-ndk --version 4.1.2 --locked
    gradleprops:
      - dexfiles.abis=arm64-v8a
      - dexfiles.unsigned=true
    ndk: 29.0.14206865

AutoUpdateMode: Version
UpdateCheckMode: Tags ^v[0-9.]+$
CurrentVersion: 0.1.0
CurrentVersionCode: 1
```

**Comprobado:** con esas opciones (`-Pdexfiles.abis=arm64-v8a -Pdexfiles.unsigned=true`, sin
`dexfiles.features`) Gradle produce `app-release-unsigned.apk` y la biblioteca no contiene UnRAR.

**Para enviarlo** hace falta una cuenta de GitLab:
1. Haz un *fork* de [fdroiddata](https://gitlab.com/fdroid/fdroiddata).
2. Añade `metadata/io.github.dexfiles.yml` con el borrador de arriba.
3. Abre la *merge request*. La plantilla de F-Droid pide confirmar la licencia y que no hay
   dependencias no libres.

Conviene enviarlo con la versión firmada con la clave definitiva ya publicada en GitHub.

Puntos que la revisión de F-Droid mirará:

- **Sin RAR.** La compilación de F-Droid no pasa `dexfiles.features=rar`, así que no lleva UnRAR,
  que no es software libre. El resto de formatos funcionan igual.
- **Permiso «Acceso a todos los archivos»** (`MANAGE_EXTERNAL_STORAGE`). F-Droid lo admite en un
  explorador de archivos; se pide en la app, explicando para qué.
- **Firma.** F-Droid firma con su propia clave, salvo que se active la verificación de builds
  reproducibles contra el APK de GitHub. La biblioteca nativa se compila sin rutas locales
  (`--remap-path-prefix`) y el APK no lleva información de dependencias ni del control de
  versiones. El APK de GitHub lleva RAR y el de F-Droid no, así que no serían idénticos: hay que
  dejar que F-Droid firme con su clave.
- **Dependencias no libres:** ninguna. GameActivity y AndroidX son Apache-2.0; no hay servicios
  de Google, analítica ni anuncios.
- **Sin firmar para F-Droid:** `-Pdexfiles.unsigned=true` produce `app-release-unsigned.apk`.
