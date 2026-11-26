<div align="center">

<img width="50%" alt="Nosih" src="https://github.com/user-attachments/assets/8c715a53-a0df-4780-9dd4-09ce3ccd9043" />

  **Un dock ligero, inspirado en macOS, para Windows 10 y 11.**

  Ampliación suave, miniaturas de ventanas en vivo y un efecto genio opcional al minimizar.

  [English](README.md) · **Español** · [Français](README.fr.md)
</div>

Nosih sustituye la barra de tareas de Windows por un dock personalizable y una
barra superior opcional al estilo de macOS. Está hecho para Windows 10 y 11 y sigue
en desarrollo activo.

<img width="1919" height="1008" alt="Nosih en Windows 11" src="https://github.com/user-attachments/assets/b6c44203-c425-43fb-adf5-e2edd2f02257" />

## Funciones

- Dock de cristal con ampliación suave de los iconos bajo el cursor.
- Elige en Ajustes → Dock → *Posición en pantalla* si el dock va a la izquierda,
  abajo o a la derecha. Abajo es la posición predeterminada.
- Abrir, fijar, reordenar y gestionar apps, incluidas las recientes y las ventanas abiertas.
- Agrupar apps fijadas en un solo icono del dock: suelta una app sobre otra, haz
  clic en el grupo para desplegar sus apps en abanico y arrastra una fuera del
  abanico para sacarla.
- Las ventanas minimizadas se guardan en el icono de su app con el efecto genio;
  activa Ajustes → Ventanas → *Mostrar ventanas minimizadas* para verlas como
  miniaturas en vivo a la derecha del dock.
- Clic en la app que ya está delante para minimizarla, como en la barra de Windows.
- Clic derecho en una app para ver su lista de saltos, con iconos, como en la
  barra de Windows: elementos anclados y recientes, sus categorías y sus tareas.
- Arrastrar y soltar archivos sobre apps, Descargas o la Papelera.
- Una carpeta de accesos directos a la derecha del dock: su contenido se abre
  como un menú, con un submenú para cada carpeta, como una barra de
  herramientas de la barra de tareas de Windows 10.
- Visibilidad del dock y de la barra superior: siempre visibles, automática o
  inteligente (se ocultan solo mientras la ventana activa llega debajo).
- Apariencia: seguir a Windows o elegir Claro, Oscuro, Cristal esmerilado,
  Transparente o Aurora; además, una galería de fondos sin conexión.
- Monitor del sistema opcional en la barra superior: CPU, memoria y velocidad
  de red.
- Una mascota en pixel art para los momentos que importan: planes de IA,
  batería, red, papelera y un saludo cada día. Elige el cangrejo de Claude, un
  gato, un zorro, un capibara, un mochi o una chica; el recordatorio de
  descanso es opcional y viene desactivado.
- Porcentaje de batería de ratones y auriculares compatibles. Se activa en
  Ajustes → Barra superior → *Mostrar batería de accesorios*; viene desactivado.
- Barra superior con la app en primer plano, hora, red, volumen y batería, el idioma
  del teclado, el micrófono/cámara en uso y los iconos ocultos de la bandeja,
  además de los botones de búsqueda y Vista de tareas si están activados en la
  barra de Windows. El botón de Widgets viene desactivado; puedes activarlo en Ajustes.
- Una isla curva en la barra superior con los límites de Claude Code, Codex,
  OpenCode Go, Z.ai GLM Coding Plan, Gemini CLI y Antigravity. Z.ai muestra
  sus límites de 5 horas y de la semana; las cuotas de Gemini Pro y Flash
  aparecen por separado cuando están disponibles.
- App de Ajustes con cambios en vivo y efecto genio configurable. Ocho idiomas:
  español, inglés, francés, chino simplificado, hindi, árabe, bengalí y portugués.
  Elige uno en el desplegable o sigue automáticamente el idioma de Windows.

El efecto genio está **activado por defecto** y se puede desactivar en Ajustes →
Ventanas. Mientras está activo, Nosih
desactiva las transiciones de Windows en las ventanas que sigue, para poder
dibujar su animación; las devuelve al cerrarse.

## Instalar

**[Descargar Nosih-Setup.exe](https://github.com/crafter-station/Nosih/releases/latest/download/Nosih-Setup.exe)**
(Windows 10 versión 1903 o posterior, o Windows 11; 64 bits) o elige una versión en
[Releases](https://github.com/crafter-station/Nosih/releases).

1. Ejecuta `Nosih-Setup.exe`. Se instala solo para tu usuario: sin permisos de
   administrador, sin asistente, en unos dos segundos.
2. Nosih es nuevo, así que SmartScreen puede no reconocerlo todavía y decir que
   protegió tu PC. Elige **Más información → Ejecutar de todas formas**. Ver la
   [política de firma de código](https://nosih.crafter.run/code-signing-policy).
3. Aparece el dock y se oculta la barra de tareas de Windows. Desde entonces Nosih
   se abre al iniciar Windows; puedes desactivarlo en Ajustes → Sistema.

Abre **Nosih** desde el menú Inicio (o clic derecho en el dock → *Ajustes de
Nosih…*) para cambiar el dock. Ajustes también busca actualizaciones y las instala.

Para salir de Nosih y volver a mostrar la barra de tareas, haz clic derecho en
el dock → *Salir de Nosih*.
Para quitarlo del todo: Configuración de Windows → Aplicaciones → Aplicaciones
instaladas → Nosih → Desinstalar; la barra vuelve y no queda nada en marcha.

## Probarlo desde el código

### Requisitos

- Windows 10 (versión 1903 o posterior) o Windows 11. En Windows 10 el dock y la
  barra superior son sólidos en lugar de cristal: Windows 10 no ofrece desenfoque
  de fondo a las apps de escritorio.
- Rust con la toolchain MSVC y las Build Tools de C++ de Visual Studio
- Node.js y pnpm

### Ejecutar el dock

Desde la raíz del repositorio:

```powershell
cargo run -p nosih
```

El dock oculta la barra de tareas mientras está en marcha. Pulsa **Ctrl+C** en la
terminal para salir; Nosih debería devolver la barra solo. Para devolverla a mano:

```powershell
cargo run -p nosih -- --restore
```

### Ejecutar Ajustes

Con el dock en marcha, abre otra terminal y ejecuta:

```powershell
cd apps\settings
pnpm install
pnpm tauri dev
```

Se abre la ventana de Ajustes. Los cambios de la interfaz se recargan solos; tras
cambiar el backend en Rust, reinicia `tauri dev`. Los cambios de Ajustes se
aplican al dock en marcha.

## Compilar y probar

Desde la raíz del repositorio:

```powershell
cargo fmt --all -- --check
cargo test --workspace --locked
```

Para compilar la interfaz de Ajustes:

```powershell
cd apps\settings
pnpm build
```

## Estructura

| Ruta | Para qué |
|---|---|
| `apps/dock` | El dock nativo de Windows |
| `apps/settings` | La app de Ajustes (Tauri) |
| `crates/nosih-core` | Lógica de disposición y animación, independiente de la plataforma |
| `crates/nosih-shell` | Integración con el shell de Windows |
| `crates/nosih-config` | Ajustes compartidos y su guardado |
| `nosih/` | La web (Astro) |

## Privacidad

Nosih no recoge datos, no tiene telemetría y nunca envía nada al proyecto. Hace
dos tipos de conexión de red, ambas opcionales:

- **Actualizaciones:** Ajustes pregunta a GitHub Releases si hay una versión
  nueva al abrirse y, una vez al día, un par de minutos después de encender el
  ordenador, Nosih hace lo mismo en segundo plano e instala lo que encuentre
  (el dock en sí nunca usa la red). La instalación se desactiva en Ajustes →
  Sistema → *Instalar actualizaciones automáticamente*, y todo con *Buscar
  actualizaciones automáticamente*.
- **Uso de proveedores de IA:** mientras un cliente compatible está abierto,
  Nosih reutiliza su sesión local para consultar los límites disponibles de
  su proveedor cada cinco minutos mientras está visible, o al pulsar Actualizar.
  Gemini CLI y Antigravity pueden mostrar por separado las cuotas de Gemini Pro
  y Flash; Antigravity se consulta mediante su servicio local. Las credenciales
  permanecen en memoria; Nosih no las guarda. Para Z.ai Coding Plan, Nosih
  lee la clave del `auth.json` local de OpenCode o de `ZAI_API_KEY` /
  `ZHIPU_API_KEY` y la envía al servicio de cuotas de Z.ai. Desactiva las
  consultas en Ajustes → Barra superior → *Mostrar uso de proveedores de IA*.
- **Notificaciones:** el contenido y los avisos de las notificaciones se procesan exclusivamente en la memoria local de tu equipo; nunca se escriben en disco, ni se guardan en registros ni se transmiten por la red.

Detalles: [nosih.crafter.run/privacy](https://nosih.crafter.run/privacy).
