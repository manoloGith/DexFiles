# Apps Dex en DexFiles

Cada app de la familia Dex (DexPdfCraft, …) dice en su manifiesto qué extensiones abre, y DexFiles
la pone como **aplicación predeterminada** para ellas (Ajustes ▸ Aplicaciones predeterminadas).

## Por qué DexFiles lo lee y no al revés

DexFiles consulta al sistema qué apps lo declaran, en vez de que cada app se lo comunique a DexFiles.

- **No hace falta que la app se ejecute.** Una app recién instalada no puede avisar hasta que se
  abre, y quizá nunca se abra antes de usarla desde DexFiles. Lo declarado en el manifiesto se ve
  nada más instalarla.
- **Funciona en cualquier orden.** Da igual instalar DexFiles antes o después de las apps.
- **Sin comunicación entre apps:** ni broadcasts, ni permisos, ni nada que mantener en cada app. Es
  una línea en su manifiesto.
- **Siempre al día.** Si una app se desinstala, deja de aparecer. DexFiles vuelve a mirar al
  arrancar y cada vez que su ventana recupera el foco (por ejemplo, al volver de instalar algo).

## Qué decide DexFiles

- Cada extensión declarada se ofrece **una sola vez**. Si nadie más la abre, se asigna a la app Dex
  y se avisa en la barra de estado.
- No sustituye a una app que hayas elegido tú para esa extensión.
- Si la quitas o la cambias, **no vuelve a ponerla**: queda anotada como ya ofrecida.
- *Ajustes ▸ Aplicaciones predeterminadas ▸ Volver a usar las apps Dex* las asigna de nuevo, sin
  tocar las extensiones que tengan otra app elegida.

## Cómo lo declara una app

### Android

En la actividad que abre los archivos (la misma que responde a `VIEW`):

```xml
<activity android:name=".MainActivity" android:exported="true" …>
    <!-- La de siempre: abrir un content:// del tipo -->
    <intent-filter>
        <action android:name="android.intent.action.VIEW" />
        <category android:name="android.intent.category.DEFAULT" />
        <data android:scheme="content" />
        <data android:mimeType="application/pdf" />
    </intent-filter>

    <!-- Para DexFiles -->
    <intent-filter>
        <action android:name="io.github.dexfiles.action.OPEN" />
        <category android:name="android.intent.category.DEFAULT" />
    </intent-filter>
    <meta-data android:name="io.github.dexfiles.extensions" android:value="pdf" />
</activity>
```

- **Extensiones:** separadas por comas, puntos y coma o espacios (`pdf, xps`), sin punto o con él.
- **Cómo abre DexFiles los archivos:** con `ACTION_VIEW`, el `content://` del archivo (su
  FileProvider), su tipo MIME y permiso de lectura y escritura, dirigido a esa actividad.
- **Visibilidad de paquetes:** DexFiles tiene la acción en su `<queries>`, así que la app no
  necesita nada más.

### Escritorio (Linux)

Para el desarrollo, la clave `X-DexFiles-Extensions` en el `.desktop` de la app:

```ini
[Desktop Entry]
Type=Application
Name=DexPdfCraft
Exec=dexpdfcraft %f
MimeType=application/pdf;
X-DexFiles-Extensions=pdf
```
