# DexFiles: plan y estado

Explorador de archivos moderno (al estilo de Finder) para **Samsung DeX** y el modo escritorio de
Android, en Rust con egui, sobre [craft-android](https://github.com/manoloGith/craft-android) igual
que DexPdfCraft: ratón (rueda, botón derecho, botones atrás/adelante), teclado físico, teclado en
pantalla, cierre y reapertura sin cuelgues, diagnóstico sin adb.

## Estado de cada requisito (fase 1)

Leyenda: ✅ hecho y probado en escritorio · 🟡 hecho, falta probarlo en el dispositivo o con un
servidor real · ⏳ fase siguiente.

### 1. Navegación y estructura

| Requisito | Estado | Cómo |
|---|---|---|
| Panel de navegación jerárquico | ✅ | Barra lateral: Favoritos, Dispositivos (cada volumen se despliega como árbol de carpetas, con barra de ocupación), Red, Papelera. Todo es destino de arrastre. |
| Unidades de red FTP/SFTP/SMB/WebDAV | ✅ FTP, WebDAV · 🟡 FTPS, SFTP, SMB | *Conectar a servidor…* o escribir `sftp://usuario@host/ruta` en la barra de direcciones. Todo Rust (suppaftp, russh + russh-sftp, smb-rs, ureq). Contraseña recordada o solo para la sesión. SFTP con *trust on first use* de la clave del host. |
| Barra de direcciones | ✅ | Migas de pan clicables (y destino de arrastre); clic en el hueco o Ctrl+L para escribir una ruta o URL; botón para copiarla. |
| Atrás / Adelante / Subir | ✅ | Botones, Alt+←/→/↑, Retroceso, botones laterales del ratón; en Android, *Atrás* del sistema vuelve atrás y solo cierra desde la primera carpeta. |
| Pestañas | ✅ | Ctrl+T, Ctrl+W, Ctrl+Tab, clic central en una carpeta, *Abrir en pestaña nueva*. Soltar archivos sobre una pestaña los lleva a su carpeta. |
| Favoritos | ✅ | Carpetas estándar al primer arranque; *Añadir a favoritos* o arrastrar una carpeta bajo la lista; reordenar y quitar desde su menú. |
| Panel de vista previa | ✅ imágenes, texto/código, DOCX/ODT/PPTX/ODP (texto), XLSX/XLS/ODS/CSV (tabla), archivos comprimidos (lista), carpetas · ⏳ PDF | Panel derecho redimensionable + *Vista rápida* con Espacio. PDF: fase 2 (con `hayro`, el renderizador de PdfCraft). |
| Dos paneles | ✅ | F3 o el botón; cada panel con sus pestañas; Tab cambia de panel; *Copiar/Mover al otro panel*. |

### 2. Gestión y operaciones

| Requisito | Estado | Cómo |
|---|---|---|
| Copiar, cortar, pegar, renombrar | ✅ | Ctrl+C/X/V, F2, menús. Entre cualquier par de ubicaciones (local ↔ servidor ↔ servidor). En segundo plano con progreso, cancelación y pregunta ante nombres repetidos (reemplazar / combinar / conservar ambos / omitir, «para todos»). |
| Eliminar: papelera o permanente | ✅ | Supr = papelera (especificación freedesktop; en Linux es la del sistema; en Android `.Trash-dexfiles` en cada volumen), Mayús+Supr = permanente (con confirmación). Papelera navegable: restaurar, eliminar, vaciar. En servidores no hay papelera: se avisa. |
| Nueva carpeta / nuevo archivo | ✅ | Ctrl+Mayús+N, menú del fondo. |
| Búsqueda y filtros | ✅ | Escribir filtra la carpeta al instante; Intro busca en subcarpetas (en segundo plano, resultados en directo). Filtros: nombre o contenido (texto, código y documentos de oficina), tipo, fecha de modificación, tamaño. Comodines `*` y `?`, sin distinguir mayúsculas ni acentos. |
| Arrastrar y soltar | ✅ | Entre paneles, sobre carpetas, pestañas, migas de pan, la barra lateral y la papelera. Mismo disco = mover, otro = copiar; Ctrl copia, Mayús mueve. Desde otras apps (DeX, pantalla dividida): se copian a la carpeta abierta. Selección por rectángulo. |
| Compresión | ✅ | Ver [compression.md](compression.md): ZIP, 7z, TAR(.gz/.bz2/.xz); RAR, propuesta. |
| Compartir | 🟡 | *Compartir…* (hoja de compartir de Android, varias apps) y *Enviar por correo* (solo apps de correo, como adjuntos). Los archivos de servidores se descargan antes. |

### 3. Visualización y personalización

| Requisito | Estado | Cómo |
|---|---|---|
| Vistas | ✅ | Iconos grandes, medianos, pequeños, lista, detalles, contenido (Ctrl+1…6). Miniaturas de imágenes en segundo plano. |
| Ordenar | ✅ | Nombre (orden natural: «2» antes de «10»), fecha, tipo, tamaño, estado (solo lectura/oculto/enlace); ascendente/descendente; carpetas primero. Clic en las columnas de *Detalles*. |
| Ocultos | ✅ | Ctrl+H o ajustes (archivos con punto y atributos ocultos/sistema de SMB). |
| Extensiones | ✅ | Ajuste *Mostrar extensiones*. Con ellas ocultas, renombrar solo cambia el nombre; con ellas visibles, se preselecciona el nombre sin la extensión y se avisa si cambia. |
| Configuración de inicio | ✅ | Carpeta de inicio, Descargas, una carpeta concreta o las pestañas de la última vez. |

Además: tema claro/oscuro/sistema, español/inglés, tamaño de la interfaz (para monitores DeX),
propiedades (con cálculo del tamaño de carpetas), «Abrir con…», copiar rutas, escritura por
teclado para saltar a un nombre, vigilancia de cambios en la carpeta abierta.

## Fases siguientes

| Fase | Contenido |
|---|---|
| 2 | Vista previa de PDF (hayro), audio/vídeo (metadatos y carátula), RAR (según elijas), .zst, archivos con contraseña, «confiar en la nueva clave» de SFTP, claves SSH, comprimir/extraer en servidores |
| 3 | Arrastrar *desde* DexFiles a otras apps (`startDragAndDrop`), portapapeles de archivos entre apps, etiquetas de color, renombrado por lotes |
| 4 | Release firmado, F-Droid/Obtainium (como DexPdfCraft fase 7), integración en craft-android de lo que sirva a otras apps (volúmenes, abrir con, compartir) |

## Qué se ha probado

- **Escritorio (Linux, Xvfb + xdotool)**: navegación, selección (clic, Mayús, Ctrl+A), las seis
  vistas, miniaturas, vista previa, doble panel, arrastrar y soltar entre paneles (movió los
  archivos), copiar/pegar (duplicado con nombre «(2)»), papelera, nueva carpeta, comprimir,
  filtro y búsqueda recursiva, ajustes persistentes.
- **Tests** (`cargo test`): copiar/mover/borrar con conflictos, papelera, los 6 formatos de
  compresión ida y vuelta, búsqueda por contenido, extracción de texto DOCX, PROPFIND de WebDAV,
  URLs, orden natural, historial.
- **Servidores reales**: FTP (pyftpdlib) y WebDAV (wsgidav) con `tests/network.rs` (crear, subir
  300 KB con «ñ» en el nombre, listar, descargar, leer el principio, renombrar, borrar).
- **Android**: compila para arm64 y se genera el APK. **No hay dispositivo en el entorno**: falta
  probarlo en DeX (ver la lista de [testing.md](testing.md)).
- **Sin probar**: SFTP y SMB contra un servidor real (el entorno no permitió arrancar sshd/samba);
  FTPS.
