# Probar DexFiles

## En el dispositivo (DeX y móvil)

Instala el APK (artefacto `dexfiles-debug-apk` de la CI, o `apps/dexfiles/android/app/build/outputs/apk/debug/`).
Si algo falla, abre el diagnóstico (Ctrl+Mayús+D o el icono «Diagnóstico DexFiles») y pégame el
informe.

1. Primer arranque: aparece el aviso **«DexFiles necesita acceso a todos los archivos»** →
   *Permitir el acceso* → activar el interruptor → volver: la lista se carga sola.
2. Abrir, cerrar con la X y volver a abrir **tres veces seguidas**.
3. Ratón: clic, doble clic, botón derecho (menú), rueda, botones atrás/adelante del ratón,
   selección por rectángulo, arrastrar a otra carpeta, al otro panel (F3), a la papelera.
4. Teclado: flechas, Intro, Retroceso, Supr, F2, Ctrl+C/X/V, Ctrl+A, Ctrl+T/W, Ctrl+L, Ctrl+F,
   escribir letras para saltar a un nombre.
5. *Abrir* una foto y un PDF (abre la app del sistema), *Abrir con…*, *Compartir…* y *Enviar por
   correo*.
6. Tarjeta SD o USB: aparece en Dispositivos; copiar a ella; Supr → la papelera.
7. Arrastrar un archivo desde «Mis archivos» (DeX) a la ventana de DexFiles: se copia a la carpeta
   abierta. Compartir una foto desde la Galería a DexFiles: igual.
8. Comprimir en ZIP y 7z, extraer, ver la lista en la vista previa.
9. Red: un NAS por SMB, un servidor SFTP, Nextcloud por WebDAV (`/remote.php/dav/files/USUARIO`).
10. Móvil: teclado en pantalla en la búsqueda y al renombrar, el botón Atrás vuelve atrás, giro.

## Tests automáticos

```bash
cargo test -p dexfiles --features desktop
```

Contra un servidor (crea y borra una carpeta `dexfiles-test-*`):

```bash
DEXFILES_TEST_SERVER=sftp://usuario:clave@nas.local/home/usuario cargo test -p dexfiles --test network -- --ignored
```

Esquemas: `ftp`, `ftps`, `sftp`, `smb` (`smb://usuario:clave@host/RECURSO`), `dav`, `davs`.

## En el escritorio

```bash
cargo run -p dexfiles --features desktop -- ~/Descargas
```
