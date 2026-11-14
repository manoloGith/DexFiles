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
| Unidades de red FTP/SFTP/SMB/WebDAV | ✅ FTP, WebDAV · 🟡 FTPS, SFTP, SMB | *Conectar a servidor…* o escribir `sftp://usuario@host/ruta` en la barra de direcciones. Todo Rust (suppaftp, russh + russh-sftp, smb-rs, ureq). Contraseña recordada o solo para la sesión. SFTP con contraseña o **clave privada** (OpenSSH/PEM, con frase de paso), *trust on first use* de la clave del host y botón **«Confiar en la nueva clave»** si cambia. |
| Barra de direcciones | ✅ | Migas de pan clicables (y destino de arrastre); clic en el hueco o Ctrl+L para escribir una ruta o URL; botón para copiarla. |
| Atrás / Adelante / Subir | ✅ | Botones, Alt+←/→/↑, Retroceso, botones laterales del ratón; en Android, *Atrás* del sistema vuelve atrás y solo cierra desde la primera carpeta. |
| Pestañas | ✅ | Ctrl+T, Ctrl+W, Ctrl+Tab, clic central en una carpeta, *Abrir en pestaña nueva*. Soltar archivos sobre una pestaña los lleva a su carpeta. |
| Favoritos | ✅ | Carpetas estándar al primer arranque; *Añadir a favoritos* o arrastrar una carpeta bajo la lista; reordenar y quitar desde su menú. |
| Barra espaciadora | ✅ | Abre el visor que toca: imágenes → visor de imágenes; audio y vídeo → reproductor a pantalla completa; lo demás → *vista rápida* a toda la ventana sobre el mismo fondo negro translúcido (PDF, texto, tablas, archivos comprimidos…), con anterior/siguiente por la carpeta. |
| Visor de imágenes | ✅ | Doble clic en una imagen (lista o vista previa): a toda la ventana sobre fondo negro translúcido; anterior, siguiente, cerrar, teclado y rueda. Si hay una app predeterminada de DexFiles activada para ese tipo, el doble clic (en la lista o en la vista previa) abre esa app, también para audio y vídeo; la barra espaciadora abre siempre los visores. |
| Archivos comprimidos como carpetas | ✅ | Doble clic en ZIP, 7z, TAR(.gz/.bz2/.xz/.zst) o RAR: se navega por dentro (solo lectura); abrir, previsualizar, copiar y arrastrar hacia fuera. |
| Reproductor | 🟡 | Pantalla completa (botón, Espacio o doble clic en un audio/vídeo): controles abajo que se ocultan con el ratón quieto, lista de reproducción a la derecha; el vídeo se encoge un poco mientras se ven, porque en Android es una superficie aparte que tapa lo que se dibuje encima. |
| Reproductor (panel) | 🟡 | En el panel de vista previa: audio y vídeo con `MediaPlayer` de Android (el vídeo, en una superficie del sistema colocada sobre su zona). Ocupa la zona de la imagen mientras reproduce el archivo seleccionado y se acopla abajo (un tercio) al cambiar de archivo. Anterior, reproducir/pausa, siguiente, barra de posición, lista (si hay varios) y cerrar. En escritorio solo hay un modo simulado para desarrollar la interfaz. |
| Panel de vista previa | ✅ imágenes, PDF (primera página, con `hayro`), texto/código, DOCX/ODT/PPTX/ODP (texto), XLSX/XLS/ODS/CSV (tabla), audio (etiquetas y carátula, `lofty`), archivos comprimidos (lista), carpetas · 🟡 vídeo (fotograma con `MediaMetadataRetriever`, solo Android) | Lo que se muestra ocupa el panel y la información va abajo. Los PDF se renderizan al tamaño exacto en píxeles con que se ven (nítidos) y se pasan de página; los archivos comprimidos se muestran como árbol. Panel derecho redimensionable + *Vista rápida* con Espacio. Miniaturas de imágenes, PDF, carátulas y vídeos. |
| Dos paneles | ✅ | F3 o el botón; cada panel con sus pestañas; Tab cambia de panel; *Copiar/Mover al otro panel*. |

### 2. Gestión y operaciones

| Requisito | Estado | Cómo |
|---|---|---|
| Copiar, cortar, pegar, renombrar | ✅ | Ctrl+C/X/V, F2, menús. Entre cualquier par de ubicaciones (local ↔ servidor ↔ servidor). En segundo plano con progreso, cancelación y pregunta ante nombres repetidos (reemplazar / combinar / conservar ambos / omitir, «para todos»). |
| Eliminar: papelera o permanente | ✅ | Supr = papelera (especificación freedesktop; en Linux es la del sistema; en Android `.Trash-dexfiles` en cada volumen), Mayús+Supr = permanente (con confirmación). Papelera navegable: restaurar, eliminar, vaciar. En servidores no hay papelera: se avisa. |
| Nueva carpeta / nuevo archivo | ✅ | Ctrl+Mayús+N, menú del fondo. |
| Búsqueda y filtros | ✅ | Escribir filtra la carpeta al instante y, tras una pausa, busca en todas las subcarpetas (asíncrona: varios hilos recorren las carpetas en paralelo con `read_dir`, sin `stat` salvo para los candidatos; resultados por lotes; 97 000 entradas en < 1 s en un PC). Filtros: nombre o contenido (texto, código y documentos de oficina), tipo, fecha de modificación, tamaño. Comodines `*` y `?`, sin distinguir mayúsculas ni acentos. |
| Arrastrar y soltar | ✅ | Entre paneles, sobre carpetas, pestañas, migas de pan, la barra lateral y la papelera. Mismo disco = mover, otro = copiar; Ctrl copia, Mayús mueve. Desde otras apps (DeX, pantalla dividida): se copian a la carpeta abierta. Selección por rectángulo. |
| Compresión | ✅ | Ver [compression.md](compression.md): ZIP y 7z (con contraseña AES-256), TAR(.gz/.bz2/.xz/.zst), .zst; RAR (extraer, con contraseña) solo en los APK de GitHub (*feature* `rar`). |
| Arrastrar a otras apps | 🟡 | Un arrastre de archivos que sale de la ventana pasa a ser un arrastre del sistema (Android): se pueden soltar en Mis archivos, el correo, un chat… Solo archivos locales. Si vuelven a soltarse en DexFiles se copian los originales. |
| Portapapeles entre apps | 🟡 | Copiar/cortar pone los archivos en el portapapeles de Android (como archivos, no solo rutas); pegar en DexFiles toma primero los archivos que otra app haya copiado. |
| Renombrado por lotes | ✅ | F2 o *Renombrar N elementos…* con varios seleccionados: reemplazar texto, añadir delante/detrás, numerar (`#` donde va el número), mayúsculas/minúsculas; vista previa con los nombres que chocan en rojo; se pueden intercambiar nombres entre ellos. |
| Compartir | 🟡 | *Compartir…* (hoja de compartir de Android, varias apps) y *Enviar por correo* (solo apps de correo, como adjuntos). Los archivos de servidores se descargan antes. |

### 3. Visualización y personalización

| Requisito | Estado | Cómo |
|---|---|---|
| Vistas | ✅ | Iconos grandes, medianos, pequeños, lista, detalles, contenido (Ctrl+1…6). **Cada carpeta recuerda su vista y su orden.** En *Detalles*, las columnas se ensanchan o estrechan arrastrando su borde (doble clic: restablecer), se ocultan desde el menú de la cabecera, y la altura de las filas es ajustable; el nombre ocupa el resto del ancho. |
| Ordenar | ✅ | Nombre (orden natural: «2» antes de «10»), fecha, tipo, tamaño (las carpetas por su contenido), estado (solo lectura/oculto/enlace); ascendente/descendente; carpetas primero. |
| Tamaño de carpetas | ✅ | Calculado en segundo plano (dos hilos, recorrido de abajo arriba que guarda el de cada subcarpeta), en caché y recalculado cuando la app cambia algo dentro. En las vistas, la vista previa y la suma de la selección. Solo carpetas locales. |
| Aplicaciones predeterminadas | ✅ escritorio · 🟡 Android | Propias de DexFiles, no del sistema: *Abrir con…* muestra las apps (con iconos en Android) y «abrir siempre .ext con esta app»; *Ajustes ▸ Aplicaciones predeterminadas* las lista, cambia, quita o añade por extensión. |
| Ajustes | ✅ | Ventana con menú lateral (General, Vista, Apariencia, Red, Aplicaciones, Etiquetas) y opciones en tarjetas; en pantallas estrechas el menú queda en iconos. Aplicaciones predeterminadas por extensión o en árbol por aplicación, cada una con un interruptor para activarla o desactivarla (y uno por aplicación para todas sus extensiones); una desactivada conserva su app pero no se usa. Se guardan en `settings.ron` en cuanto cambian (escritura atómica, al perder el foco y al salir): una actualización o un cierre forzado no pierde nada; un archivo ilegible se aparta como `settings.ron.bad-…`; las versiones anteriores (`app.ron`) se leen al actualizar. |
| Etiquetas propias | ✅ | *Ajustes ▸ Etiquetas*: crear, renombrar, cambiar el color (selector), ordenar y eliminar (pide confirmación si hay elementos con ella). Las siete de colores son las iniciales y conservan lo etiquetado. |
| Etiquetas de color | ✅ | Siete colores en el menú contextual (puntos en la esquina del icono); cada color en la barra lateral lista lo que lo lleva, esté donde esté; soltar archivos sobre un color los etiqueta. Siguen al archivo al renombrarlo o moverlo desde DexFiles. |
| Apps Dex | ✅ escritorio · 🟡 Android | Cada app Dex declara en su manifiesto las extensiones que abre y DexFiles la pone como app predeterminada para ellas, una vez (si la quitas no vuelve). Ver [apps-dex.md](apps-dex.md). DexPdfCraft ya lo declara (`.pdf`). |
| Ocultos | ✅ | Ctrl+H o ajustes (archivos con punto y atributos ocultos/sistema de SMB). |
| Extensiones | ✅ | Ajuste *Mostrar extensiones*. Con ellas ocultas, renombrar solo cambia el nombre; con ellas visibles, se preselecciona el nombre sin la extensión y se avisa si cambia. |
| Configuración de inicio | ✅ | Carpeta de inicio, Descargas, una carpeta concreta o las pestañas de la última vez. |

Además: tema claro/oscuro/sistema, español/inglés, tamaño de la interfaz (para monitores DeX),
propiedades (con cálculo del tamaño de carpetas), «Abrir con…», copiar rutas, escritura por
teclado para saltar a un nombre, vigilancia de cambios en la carpeta abierta.

## Fases siguientes

| Fase | Contenido |
|---|---|
| 2 ✅ | Memoria de vista por carpeta, columnas redimensionables, tamaño de carpetas, búsqueda paralela mientras se escribe, apps predeterminadas propias, PDF, audio y vídeo, RAR (*feature*), .zst, contraseñas en archivos, claves SSH y «confiar en la nueva clave» de SFTP. Pendiente de la fase 2: comprimir/extraer directamente en servidores (ahora se hace en local) |
| 3 ✅ | Arrastrar *desde* DexFiles a otras apps (`startDragAndDrop`), portapapeles de archivos entre apps, etiquetas de color, renombrado por lotes; Espacio abre el visor que toca y vista rápida a pantalla completa |
| 4 ✅ | Release en GitHub (workflow `release.yml`, firma con los secretos `DEXFILES_*`; ver [releasing.md](releasing.md)), Obtainium, metadatos de F-Droid y borrador de `fdroiddata` ([fdroid.md](fdroid.md)), apps Dex ([apps-dex.md](apps-dex.md)) |
| 5 🟡 | Integrar en craft-android lo que sirva a otras apps: diseño y texto para su hilo en [fase5-craft-android.md](fase5-craft-android.md); falta hacerlo allí y adaptar DexFiles. F-Droid: compilación sin RAR comprobada; falta abrir la *merge request* en fdroiddata ([fdroid.md](fdroid.md)) |

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
