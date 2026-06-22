# Guía de instalación — Plugin IBM Informix para Tabularis

Pasos para instalar y **activar** el plugin de Informix en Tabularis y que aparezca
en el selector de "Nueva conexión".

---

## 1. Requisito previo: IBM Informix Client SDK (CSDK)

El plugin se conecta a Informix a través del **driver ODBC del CSDK**, así que el
CSDK debe estar instalado en cada equipo.

> ⚠️ **El bitness debe coincidir.** Un proceso solo puede cargar un driver ODBC de
> su misma arquitectura. Revisa la arquitectura de tu driver Informix instalado:
> - Driver de **32 bits** (típico, en `C:\Program Files (x86)\IBM Informix Client SDK`) → usa el binario **`win-x86`**.
> - Driver de **64 bits** → usa el binario **`win-x64`**.
>
> Tabularis es de 64 bits pero habla con el plugin por stdio, así que el bitness
> del plugin es independiente del de la app.

---

## 2. Instalar el plugin

1. Descarga el ZIP que corresponda a tu bitness desde
   [Releases](https://github.com/danielnuld/tabularis-informix-plugin/releases):
   - `tabularis-informix-plugin-win-x86-vX.Y.Z.zip` (driver de 32 bits)
   - `tabularis-informix-plugin-win-x64-vX.Y.Z.zip` (driver de 64 bits)
2. Crea la carpeta `informix` dentro del directorio de plugins de Tabularis y
   extrae ahí el contenido del ZIP (`manifest.json` + el `.exe`):

   ```
   %APPDATA%\debba\tabularis\data\plugins\informix\
   ├── manifest.json
   └── tabularis-informix-plugin.exe
   ```

   > Ruta exacta en el Explorador: pega `%APPDATA%\debba\tabularis\data\plugins`
   > en la barra de direcciones. (Es la ruta con `debba`, **no** `%APPDATA%\tabularis`.)
3. **Reinicia Tabularis** para que descubra el plugin.

---

## 3. Activar el plugin por la interfaz (UI)

1. Abre **Settings** (icono de engranaje ⚙️).
2. Ve a la pestaña **Plugins**.
3. Haz clic en el filtro **"Instalados"** (arriba de la lista, entre "Todos" y
   "Actualizaciones").

   > ⚠️ **Importante:** la pestaña abre por defecto en **"Todos"**, que solo
   > muestra plugins **descargables del registro oficial** — ahí **no** aparece
   > Informix (aún no está publicado en el registro), por lo que **no verás
   > ningún toggle**. El toggle está únicamente en **"Instalados"**.
4. Localiza la tarjeta **IBM Informix** (los drivers integrados como MySQL muestran
   una etiqueta "Built-in"; los externos como Informix muestran el toggle).
5. Activa el **interruptor (toggle)** de la tarjeta. Cuando queda **azul/activado**,
   el driver se carga al instante (no hace falta reiniciar de nuevo).

   > 💡 **Si el toggle ya se ve activado pero "IBM Informix" no aparece en Nueva
   > conexión:** desactívalo y vuelve a activarlo **una vez**. El primer clic apaga,
   > el segundo enciende; esto registra la activación de forma explícita.

---

## 4. Crear una conexión

1. **Nueva conexión** → tipo de base de datos **IBM Informix**.
2. Pestaña **General**:
   - **Host**: `direccion_o_ip@nombre_servidor_informix`
     (ej. `192.0.2.10@ol_informix1210`). El nombre después de `@` es el
     *dbservername* (INFORMIXSERVER), distinto de la IP.
   - **Port**: el puerto del listener Informix (onsoctcp).
   - **Username / Password**.
3. Pestaña **Databases** → **Load databases** → selecciona las bases que quieras
   consultar (el plugin lista todas las del servidor).

> Para un servidor distinto, repite con su propia `IP@dbservername`.

---

## 5. Notas importantes

- **Edición en la grilla:** Tabularis identifica la fila por **una sola** columna
  de llave primaria. En tablas con **PK compuesta** el plugin **bloquea** el
  UPDATE/DELETE si afectaría más de una fila (mensaje de error claro). Para esos
  casos, edita desde el **editor SQL** con un `WHERE` que cubra todas las columnas
  de la llave.
- **Solo lectura por defecto seguro:** consulta y navegación funcionan sin riesgo.

---

## Alternativa: activar editando la config (sin UI)

Útil para automatizar la instalación en varios equipos o si el toggle da problemas.

1. **Cierra Tabularis por completo.** ⚠️ Si queda abierto, al cerrarse sobrescribe
   el archivo y pierde el cambio.
2. Abre el archivo de configuración:

   ```
   %APPDATA%\tabularis\config.json
   ```

   > Ojo: el **config** está en `%APPDATA%\tabularis\` (sin `debba`); los
   > **archivos del plugin** van en `%APPDATA%\debba\tabularis\data\plugins\`.
   > Son carpetas distintas.
3. Busca la clave `activeExternalDrivers` y agrega `"informix"`:

   | Estado actual | Cómo debe quedar |
   |---|---|
   | `"activeExternalDrivers": null` | `"activeExternalDrivers": ["informix"]` |
   | `"activeExternalDrivers": ["otro"]` | `"activeExternalDrivers": ["otro", "informix"]` |
   | (la clave no existe) | agrégala: `"activeExternalDrivers": ["informix"],` |

4. Guarda el archivo (verifica que siga siendo **JSON válido**: comas correctas,
   sin coma colgante al final).
5. Abre Tabularis. "IBM Informix" aparecerá en el selector de Nueva conexión.

---

## Solución de problemas

### Error `IM002 — No se encuentra el nombre del origen de datos y no se especificó ningún controlador predeterminado`

El administrador ODBC no encontró el driver de Informix para la arquitectura del
plugin. Ejecuta este diagnóstico en **PowerShell** para ver los 3 datos clave:

```powershell
$p = "$env:APPDATA\debba\tabularis\data\plugins\informix\tabularis-informix-plugin.exe"
"=== Driver Informix registrado en 64-bit ==="
(Get-ItemProperty 'HKLM:\SOFTWARE\ODBC\ODBCINST.INI\ODBC Drivers' -EA SilentlyContinue).PSObject.Properties |
  Where-Object { $_.Name -match 'informix' } | ForEach-Object { $_.Name }
"=== Driver Informix registrado en 32-bit (WOW6432Node) ==="
(Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\ODBC\ODBCINST.INI\ODBC Drivers' -EA SilentlyContinue).PSObject.Properties |
  Where-Object { $_.Name -match 'informix' } | ForEach-Object { $_.Name }
"=== Bitness del plugin instalado ==="
if (Test-Path $p) {
  $fs=[IO.File]::OpenRead($p);$br=New-Object IO.BinaryReader($fs);$fs.Seek(0x3C,0)|Out-Null;$pe=$br.ReadInt32();$fs.Seek($pe+4,0)|Out-Null;$m=$br.ReadUInt16();$br.Close()
  switch($m){0x14c{"plugin = 32-bit (usar win-x86)"}0x8664{"plugin = 64-bit (usar win-x64)"}default{("machine=0x{0:X}" -f $m)}}
} else { "No se encontró el plugin en $p" }
```

Interpreta el resultado:

| Resultado | Causa | Solución |
|---|---|---|
| No aparece "informix" en ningún hive | El CSDK no está instalado | Instalar el **IBM Informix Client SDK** |
| Driver en **32-bit** y plugin **64-bit** | Bitness no coincide | Reemplazar por el binario **`win-x86`** |
| Driver en **64-bit** y plugin **32-bit** | Bitness no coincide | Reemplazar por el binario **`win-x64`** |
| El nombre del driver **no** es exactamente `IBM INFORMIX ODBC DRIVER` | El setting `driver_name` no coincide | En **Settings → Plugins → ⚙ IBM Informix**, pon el nombre **exacto** en `driver_name` |

> Regla de oro: **el bitness del binario del plugin debe coincidir con el del
> driver ODBC de Informix instalado**, no con el de Tabularis.

### Error `-908 ... server (X) failed`
El dbservername (la parte después de `@` en el Host) no coincide con el
`DBSERVERNAME` real del servidor.

### Error `-25580 System error in network function`
No hay alcance de red a esa IP:puerto (puerto equivocado, firewall o VPN).
