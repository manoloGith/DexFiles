# Publicar una versión

1. **Sube la versión.**
   - En `apps/dexfiles/android/gradle.properties`: `dexfiles.version` y `dexfiles.versionCode`. El código de versión debe crecer siempre.
   - En `Cargo.toml` (`[workspace.package]`): `version`.
   - Ejecuta `cargo update -w` para que `Cargo.lock` recoja la versión nueva (la CI usa `--locked`).
2. **Escribe las notas** en `docs/release-notes/v<versión>.md`, y una línea en
   `fastlane/metadata/android/{en-US,es-ES}/changelogs/<versionCode>.txt`.
3. **Publica:** haz commit y sube un tag `v<versión>`, o lanza a mano el workflow **Release**
   (Actions ▸ Release ▸ Run workflow), que crea el tag con la versión de `gradle.properties`:

```bash
git tag v0.1.0 && git push origin v0.1.0
```

El workflow `.github/workflows/release.yml` compila el APK (arm64-v8a, **con RAR**), lo firma y
publica el release con el APK y `SHA256SUMS.txt`.

## La clave de firma

Android solo instala una actualización si está firmada con la misma clave que la versión
instalada. La clave **no está en el repositorio**: el workflow la lee de tres secretos
(Settings ▸ Secrets and variables ▸ Actions).

| Secreto | Contenido |
|---|---|
| `DEXFILES_KEYSTORE_BASE64` | el archivo `.jks` en base64 (`base64 -w0 dexfiles-release.jks`) |
| `DEXFILES_KEYSTORE_PASSWORD` | la contraseña del almacén (en un almacén PKCS12 es también la de la clave) |
| `DEXFILES_KEY_ALIAS` | el alias de la clave (si no existe en el almacén, se usa su única clave) |

**Una clave nueva:**

```bash
keytool -genkeypair -keystore dexfiles-release.jks -alias dexfiles -keyalg RSA -keysize 4096 -validity 36500 -dname "CN=DexFiles"
```

**La misma clave que DexPdfCraft:** se pueden copiar sus secretos con estos nombres. Las apps
quedan firmadas por el mismo autor.

**Sin los secretos**, el workflow firma con una clave de depuración y publica una *pre-release*.
Una pre-release instalada no se puede actualizar con versiones firmadas con la clave real: hay que
desinstalarla primero. Guarda la clave y su contraseña en un lugar seguro, con copia: si se
pierden, ninguna versión futura podrá actualizar las instaladas.

**Si se añaden los secretos después de publicar una pre-release**, vuelve a lanzar el workflow
(Actions ▸ Release ▸ Run workflow) con la misma versión. Sustituye el APK del release, pero se
queda marcado como pre-release: quita la marca a mano editando el release en GitHub.
