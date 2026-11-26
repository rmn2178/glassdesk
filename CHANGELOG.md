# Changelog / Cambios

Each release lists its changes in English and in Spanish. The release workflow
copies the section of the version being published into the GitHub release.

Cada versión lista sus cambios en inglés y en español. El flujo de publicación
copia la sección de la versión que se publica a la release de GitHub.

## [0.1.25]

### English

- Store apps show their logo without the coloured square behind it, as their
  taskbar button does (issue #73): Nosih uses the package's own logo, its light
  form on a light taskbar, and the shell's image only for apps without one.
- Apps' unread indicators show on their Dock icons, as on the taskbar (issue
  #65): the red dot or count Slack, Zoom or Teams put on their taskbar button
  appears at the top right of their icon, in place of the attention dot. Nosih
  follows Windows' "Show badges on taskbar apps" setting, without a separate
  switch in Settings, and returns overlay handling to Explorer when badges are
  disabled or Nosih exits. An app already open when Nosih starts may need a
  restart to show it.
- Release verification now also checks the final installer with Windows
  Attachment Services using its HTTPS download origin. Rejection, unavailable
  verification or changed/removed bytes stop publication before updater metadata
  is produced. This does not replace Microsoft's review of antivirus detections.

### Español

- Las apps de la Tienda muestran su logo sin el cuadrado de color detrás, como
  su botón de la barra de tareas (issue #73): Nosih usa el logo del paquete,
  su forma clara con la barra clara y la imagen del shell solo para las apps
  que no tienen logo propio.
- Los indicadores de mensajes sin leer de las apps se ven en sus iconos del
  Dock, como en la barra de tareas (issue #65): el punto rojo o el número que
  Slack, Zoom o Teams ponen en su botón aparece arriba a la derecha de su
  icono, en lugar del punto de atención. Nosih sigue la configuración de
  Windows «Mostrar distintivos en las aplicaciones de la barra de tareas», sin
  un interruptor separado en Ajustes, y devuelve a Explorer la gestión de las
  insignias al desactivarlas o cerrar Nosih. Una app ya abierta al iniciar
  Nosih puede necesitar reiniciarse para mostrarlo.
- La verificación de releases comprueba además el instalador final con Windows
  Attachment Services y su origen HTTPS de descarga. Un rechazo, una comprobación
  no disponible o bytes cambiados/eliminados detienen la publicación antes de
  generar metadatos del actualizador. No sustituye la revisión de Microsoft.

## [0.1.24]

### English

- Liquid Glass replaces Aurora in Appearance for the Dock and menu bar (#67).
  Neutral edge reflections and a brighter rim approximate curved glass with
  static compositor layers, without capture or continuous animation. Frosted
  glass is unchanged. Both Windows palettes and the opaque energy/transparency
  fallback are supported; saved Aurora choices migrate to Liquid Glass.
- Five more mascots: a tabby cat, a red fox, a capybara, a mochi (a white blob
  whose mitten hands move with its mood) and a chibi girl with a bun, whose
  hands also follow her mood, in pixel art. Each has the same five expressions
  and animations as Claude's crab: happy, in love, sad with a tear, asleep, and
  walking across the Dock. Choose one under Settings > Dock; each preview uses
  its own dock sprites over a backdrop of its world. Claude's crab remains the
  default, and the website showcases all six.
- The mascot comes out for more moments, each where it belongs: under the
  battery icon when it falls to 20 % or 10 % on battery or is full while
  charging; under the network icon when the connection has been lost for a few
  seconds (brief drops say nothing) and when it is back; on the recycle bin
  when it is emptied; across the Dock to say hello the first time each day
  (after start, waking up or unlocking); in the middle of the Dock after an
  update. Without the menu bar on screen, its moments come to the Dock. An
  optional break reminder (Settings > Dock, off by default) has it yawn after
  90 minutes of work without a pause. All from events the dock already
  receives; only the reminder adds a timer, once a minute, while it is on.

### Español

- Liquid Glass sustituye a Aurora en Apariencia para el Dock y la barra (#67).
  Reflejos neutros y un borde más definido aproximan el vidrio curvo mediante
  capas estáticas del compositor, sin capturas ni animación continua. Se conserva
  el esmerilado. Respeta ambas paletas de Windows y el modo opaco por energía o
  transparencia; las preferencias de Aurora migran a Liquid Glass.
- Cinco mascotas más: un gato atigrado, un zorro, un capibara, un mochi (una
  bolita blanca que mueve las manos según su ánimo) y una chica chibi con moño,
  cuyas manos también siguen su ánimo, en pixel art. Cada una comparte las cinco
  expresiones y animaciones del cangrejo de Claude: alegría, amor, tristeza con
  una lágrima, sueño y paseo por el Dock. Se eligen en Ajustes > Dock y tienen
  una vista previa con sus sprites sobre el fondo de su mundo. El cangrejo de
  Claude sigue siendo la opción predeterminada; la web muestra las seis.
- La mascota aparece en más momentos, cada uno en su sitio: bajo el icono de
  la batería cuando baja al 20 % o al 10 % sin cargador o se llena cargando;
  bajo el icono de red cuando la conexión lleva unos segundos perdida (los
  cortes breves no dicen nada) y cuando vuelve; sobre la papelera al vaciarla;
  cruzando el Dock para saludar la primera vez de cada día (al iniciar, al
  despertar o al desbloquear); en el centro del Dock tras una actualización.
  Sin la barra superior en pantalla, sus momentos van al Dock. Un recordatorio
  de descanso opcional (Ajustes > Dock, desactivado por defecto) la hace
  bostezar tras 90 minutos de trabajo sin pausa. Todo con eventos que el dock
  ya recibe; solo el recordatorio añade un temporizador, cada minuto, mientras
  está activo.

## [0.1.23]

### English

- Frosted glass now adapts to Windows' light/dark shell palette, with a lighter
  tint that reveals the backdrop while keeping text panels readable (#60).
  Material presets no longer force Settings into light or dark mode; explicit
  Light/Dark choices still work. Windows transparency and energy policies apply.
- Settings now offers 20 wallpapers in four masonry columns. Only small offline
  previews are bundled; full-resolution images download from GitHub on selection,
  are verified and cached for offline reuse. Opening the gallery does not fetch
  originals, and download failures preserve the current wallpaper.
- Changing a wallpaper from Settings now notifies the Dock directly and renews
  existing glass surfaces without waiting for a theme change. Wallpaper and DWM
  notifications preserve open previews and player controls when the theme is
  unchanged; duplicate events are coalesced without polling.
  The gallery uses Windows' persisted desktop setting and shell notification to
  reduce the interval where acrylic still shows the previous image.
- The menu bar can have a theme of its own (issue #54): turn off "Same theme
  for the menu bar" in Settings → Appearance and pick one for the menu bar and
  its panels, independently of the dock. By default it still shares the
  dock's theme, and existing settings are unchanged.
- Release now scans the final Dock, Settings and installer bytes with Microsoft
  Defender before creating updater metadata or a draft release, and retains
  hashes, signature details and scan evidence. Detections and incomplete scans
  stop publication; runtime app behavior is unchanged.
- Fixed the menu bar's "^" button starting the first hidden tray app right
  after opening the panel (issue #52). It now opens it with Space instead of
  Enter, whose key-up reached the panel's first icon, and sends no key unless
  the taskbar actually took the focus.
- Windows no longer open or stay with their title bar under the menu bar
  (issue #53): one that comes to the front, or is let go of after a drag, with
  its top edge under the visible bar moves just below it, as on macOS. Firefox
  popups from `window.open()` opened at the very top and lost their close
  button. Maximized, full-screen and borderless windows are left alone, as is
  everything while the bar auto-hides. Event-driven, no polling.
- Long folder lists in the Dock can be scrolled again (issue #56). Windows let
  them grow to the whole screen, where the Dock and the menu bar covered their
  scroll arrows; each list now fits between the two, so both arrows show and
  scroll it on hover, as in any Windows menu.
- Optional system monitor in the menu bar (issue #64), under Settings > Menu
  bar: CPU and memory over upload and download speed, in two small columns
  left of the status icons; clicking it opens Task Manager. Off by default.
  While on, three cheap kernel queries every 2 s, only while the bar is on
  screen: nothing while it is hidden, auto-hidden or under a full-screen app.
- Menu bar visibility is now one choice, as the Dock's (issue #61): Always
  visible, Automatic or the new Intelligent, which shows the bar unless the
  active window reaches under it, with the top edge revealing it meanwhile.
  Intelligent reserves no strip of the screen, so maximized windows use the
  full height. Existing choices are kept; it reuses the window events the dock
  already follows, with no polling.
- Opening an app that already runs in the notification area (Rainmeter, issue
  #59) now bounces its icon twice instead of up to 10 s: such apps may open no
  window when started again. One process lookup per click, only for apps
  without windows.

### Español

- El cristal esmerilado sigue el modo claro/oscuro de Windows, con menos tinte
  para dejar ver el fondo y paneles legibles (#60). Los materiales ya no fuerzan
  Ajustes a claro u oscuro; las opciones Claro/Oscuro explícitas se conservan.
  Se respetan las políticas de transparencia y energía de Windows.
- Ajustes ofrece 20 fondos en cuatro columnas masonry. Solo las miniaturas van
  incluidas; los originales se descargan de GitHub al elegirlos, se verifican y
  quedan disponibles sin conexión. Abrir la galería no descarga originales y un
  fallo de descarga conserva el fondo actual.
- Cambiar el fondo desde Ajustes ahora avisa directamente al Dock y renueva el
  cristal existente sin esperar un cambio de tema. Las notificaciones del fondo
  y de DWM conservan las previsualizaciones y controles abiertos si el tema no
  cambia; los eventos repetidos se agrupan sin consultas periódicas.
  La galería usa el ajuste persistente del escritorio y su notificación nativa
  para reducir el intervalo en que el cristal muestra la imagen anterior.
- La barra superior puede tener su propio tema (issue #54): desactiva "Mismo
  tema para la barra superior" en Ajustes → Apariencia y elige uno para la
  barra y sus paneles, independiente del dock. Por defecto sigue usando el
  tema del dock, y los ajustes existentes no cambian.
- Release analiza los binarios finales del Dock, Ajustes y el instalador con
  Microsoft Defender antes de crear el manifiesto del actualizador o el borrador,
  conservando hashes, firmas y el resultado. Las detecciones o análisis fallidos
  detienen la publicación; no cambia el funcionamiento de la app.
- Corregido el botón «^» de la barra superior, que abría el primer icono oculto
  de la bandeja justo después de mostrar el panel (issue #52). Ahora lo abre
  con Espacio en vez de Enter, cuya tecla al soltarse llegaba al primer icono
  del panel, y no envía ninguna tecla si la barra de tareas no recibió el foco.
- Las ventanas ya no se abren ni se quedan con la barra de título bajo la barra
  superior (issue #53): la que pasa al frente, o se suelta tras arrastrarla, con
  su borde superior bajo la barra visible baja justo debajo, como en macOS. Los
  popups de Firefox con `window.open()` se abrían arriba del todo y perdían el
  botón de cerrar. No se tocan las ventanas maximizadas, a pantalla completa ni
  sin bordes, ni nada mientras la barra se oculta sola. Por eventos, sin sondeos.
- Las listas largas de las carpetas del Dock vuelven a poder desplazarse (issue
  #56). Windows las dejaba crecer hasta ocupar toda la pantalla, donde el Dock y
  la barra superior tapaban sus flechas de desplazamiento; ahora cada lista cabe
  entre ambos, así que las dos flechas se ven y la desplazan al pasar el ratón,
  como en cualquier menú de Windows.
- Monitor del sistema opcional en la barra superior (issue #64), en Ajustes >
  Barra superior: CPU y memoria sobre la velocidad de subida y bajada, en dos
  columnas pequeñas a la izquierda de los iconos de estado; al hacer clic abre
  el Administrador de tareas. Desactivado por defecto. Activado, hace tres
  consultas baratas al sistema cada 2 s y solo con la barra en pantalla: nada
  mientras está oculta, se oculta sola o hay una app a pantalla completa.
- La visibilidad de la barra superior es ahora una sola elección, como la del
  Dock (issue #61): Siempre visible, Automática o la nueva Inteligente, que
  muestra la barra salvo cuando la ventana activa llega debajo de ella; mientras
  tanto, el borde superior la muestra. Inteligente no reserva franja de
  pantalla, así que las ventanas maximizadas usan toda la altura. Se conservan
  las elecciones guardadas; usa los eventos de ventana que el dock ya sigue,
  sin sondeos.
- Abrir una app que ya está en marcha en el área de notificaciones (Rainmeter,
  issue #59) hace rebotar su icono dos veces en vez de hasta 10 s: estas apps
  pueden no abrir ninguna ventana al iniciarlas de nuevo. Una consulta de
  procesos por clic, solo para apps sin ventanas.

## [0.1.22]

### English

- Added Simplified Chinese, Hindi, Arabic, Bengali and Portuguese to Settings,
  Dock menus and menu-bar panels. Language now uses a dropdown with native
  language names and an Automatic option that follows Windows. Arabic Settings
  use a right-to-left layout; native labels respect their text's reading direction.
  The website's Settings preview now offers the same languages and RTL layout.
- Added an offline wallpaper gallery to Appearance with 12 desktop backgrounds.
  Selecting an image applies it through Windows; opening the gallery leaves the
  current background untouched. Small lazy-loaded previews, serialized background
  work and embedded originals keep the gallery independent of network access.
- Intelligent hiding now measures only the visible part of the active window:
  its invisible resize borders reaching into the Dock no longer hide it.
- The Genie minimize animation now defaults to Fast (0.35 s). Saved speed
  choices remain unchanged.
- Fixed the "Show hidden icons" tooltip appearing over the desktop after closing
  a full-screen app or the last window (issue #48). Windows gave the focus to
  the hidden taskbar; the dock now hands it to the window in front, or to the
  desktop. The menu bar's hidden-icons button keeps working.
- Window previews always have their close button, and icons always bounce when
  an app asks for attention: both Settings switches were removed and their
  saved values are ignored.
- New Appearance section in Settings (issue #50): the dock, the menu bar and
  their menus and panels follow Windows by default (Automatic) or take one of
  five themes: Light, Dark, Frosted glass, Clear and Aurora, each previewed over
  your own wallpaper. Glass themes fall back to solid while Windows transparency
  is off. Settings follows the chosen theme too. No new timers or polling.
- Dock visibility now uses one dropdown: Always visible, Automatic or
  Intelligent. New installs and Reset use Intelligent; existing choices are
  preserved. Fixed policy transitions and pending input; menus and held clicks
  keep the Dock open, and release/cancellation resumes the existing hide delay.
  The context-menu toggle also keeps the selected policies mutually exclusive.
- Internal: split the dock coordinator into private modules by responsibility,
  keeping its shared state, event entry points, timers and behavior unchanged.
- Battery percentage, Search, Task View and Widgets now follow Windows settings
  directly. Removed their duplicate Settings switches and retired saved overrides.
  Search changes also update through registry notifications; no new polling.
- Bluetooth now follows Windows automatically: its menu-bar icon appears only
  while a Bluetooth radio is on. Removed the duplicate Settings toggle and
  retired saved overrides. State/hotplug events update the icon without polling.
- Fixed window previews disappearing while moving diagonally from the Dock icon
  towards an outer card. The handoff area follows the picker width on every Dock
  edge and delayed mouse-leave messages no longer cancel a newer hover.
- The music/video popup now shows playback progress and elapsed/total time when
  provided by the player. The speaker button on its right opens a vertical volume
  capsule beside the player, with mute at the bottom. Timeline events and the
  local timer stop when hidden; progress updates retain the existing visual tree.
- Added an opt-in setting under Settings > Windows to show other workspaces. With
  compatible shell-cloaking window managers (such as komorebi's Cloak mode),
  clicking a pinned app can reuse its existing window instead of launching
  another instance. Also includes other Windows virtual desktops; app-cloaked
  and inherited-cloak windows remain excluded. No additional polling.

### Español

- Añadidos chino simplificado, hindi, árabe, bengalí y portugués en Ajustes,
  menús del Dock y paneles de la barra superior. El idioma se elige en un
  desplegable con nombres nativos y Automático para seguir Windows. Ajustes
  admite distribución de derecha a izquierda en árabe; las etiquetas nativas
  respetan el sentido de lectura de su texto. La vista previa de Ajustes en la
  web ofrece los mismos idiomas y la distribución RTL.
- Galería de 12 fondos de pantalla sin conexión en Apariencia. Elegir una imagen
  la aplica mediante Windows; abrir la galería conserva el fondo actual.
  Miniaturas ligeras bajo demanda, trabajo en segundo plano y originales
  incluidos en Ajustes, sin dependencia de conexión ni consultas periódicas.
- La ocultación inteligente mide solo la parte visible de la ventana activa:
  sus bordes invisibles de redimensionado ya no ocultan el Dock al rozarlo.
- El efecto genio al minimizar ahora viene en velocidad Rápida (0,35 s).
  Se conservan las velocidades guardadas.
- Corregido el globo «Mostrar iconos ocultos» que aparecía sobre el escritorio
  al cerrar una app a pantalla completa o la última ventana (issue #48). Windows
  daba el foco a la barra de tareas oculta; ahora el dock lo pasa a la ventana
  del frente o al escritorio. El botón de iconos ocultos de la barra superior
  sigue funcionando.
- Las previsualizaciones de ventanas siempre tienen su botón de cerrar y los
  iconos siempre rebotan cuando una app pide atención: se quitaron ambos
  interruptores de Ajustes y se ignoran sus valores guardados.
- Nueva sección Apariencia en Ajustes (issue #50): el dock, la barra superior y
  sus menús y paneles siguen a Windows por defecto (Automático) o usan uno de
  cinco temas: Claro, Oscuro, Cristal esmerilado, Transparente y Aurora, cada uno
  con su vista previa sobre tu propio fondo de pantalla. Los temas de cristal se
  ven sólidos mientras Windows tenga la transparencia desactivada. Ajustes
  también sigue el tema elegido. Sin temporizadores ni sondeos nuevos.
- Visibilidad del Dock en un único desplegable: Siempre visible, Automática
  o Inteligente. Las instalaciones nuevas y Restablecer usan Inteligente;
  se conservan las elecciones anteriores. Corregidas las transiciones y
  pulsaciones pendientes; menús y pulsaciones mantienen el Dock abierto y al
  terminar se retoma el retraso existente. El menú contextual también evita
  activar ambas políticas a la vez.
- Interno: separado el coordinador del dock en módulos privados por
  responsabilidad, conservando estado, eventos, temporizadores y comportamiento.
- Porcentaje de batería, Búsqueda, Vista de tareas y Widgets siguen directamente
  los ajustes de Windows. Retirados sus interruptores duplicados y preferencias
  antiguas. Búsqueda también se actualiza por eventos del registro, sin sondeo.
- Bluetooth sigue automáticamente a Windows: su icono aparece solo cuando hay
  una radio Bluetooth encendida. Retirados el interruptor duplicado de Ajustes
  y las preferencias antiguas. Los eventos de estado/hardware actualizan el
  indicador sin sondeo periódico.
- Corregido el cierre de previsualizaciones al ir en diagonal desde el icono
  hacia una miniatura lateral. La zona de paso se adapta al selector en cada
  borde del Dock; las salidas tardías del ratón no cancelan un hover más reciente.
- El popup de música/vídeo muestra el progreso y los tiempos transcurrido/total
  cuando el reproductor los proporciona. El icono de audio a la derecha abre una
  cápsula de volumen vertical junto al panel, con silencio debajo. Se detiene el
  seguimiento y el temporizador al ocultar el popup, sin reconstruirlo en cada avance.
- Añadida una opción desactivada por defecto en Ventanas para mostrar ventanas
  de otros espacios de trabajo. Con gestores compatibles (como el modo Cloak
  de komorebi), permite reutilizar la ventana de una app fijada en vez de abrir
  otra instancia. También incluye otros escritorios virtuales de Windows;
  siguen excluidas las ventanas ocultas por la app o por herencia. Sin sondeo nuevo.

## [0.1.21]

### English

- Removed Nosih's custom notification cards, notification CLI/IPC and banner
  settings. Windows notifications remain managed by Windows; native dock
  attention indicators and bounce are retained.
- Added optional intelligent Dock hiding, disabled by default. When enabled,
  maximized windows use the full work area and the Dock hides when the active
  app overlaps it. Enable it independently of always-auto-hide in Settings.
- Claude Desktop background processes no longer imply active use. Native Claude
  Code is distinguished from Desktop, and missing credentials name the required
  Claude Code sign-in instead of promising Desktop authentication works.
- Recreate dock/menu-bar backdrop materials on Windows theme/colorization changes,
  even when cached light/glass flags are unchanged.

### Español

- Retirados los banners personalizados, su CLI/IPC y sus ajustes. Windows
  conserva sus notificaciones; permanecen el indicador de atención y el rebote
  del Dock.
- Añadida ocultación inteligente del Dock, desactivada por defecto. Al activarla,
  las ventanas maximizadas aprovechan el área útil completa y el Dock se oculta
  si la app activa ocupa su espacio, independientemente del auto-hide.
- Claude Desktop cerrado en segundo plano ya no cuenta como uso activo. Se
  distingue el cliente nativo de Claude Code de Desktop y, sin credenciales,
  se indica cómo iniciar sesión en Claude Code.
- Los cambios de tema/color del compositor reconstruyen el cristal del Dock y
  la barra, aunque las opciones de claro/oscuro y transparencia no hayan cambiado.

## [0.1.20]

### English

- Energy-saver changes now update the shared transparency state directly from
  Windows power callbacks before redrawing the dock and menu bar, with an
  initial state notification and a fallback for older systems. No idle polling.
- Dock unread counts now take precedence over generic attention dots, avoiding
  overlapping badges. Counts update and clear without rebuilding the dock.
- Fixed the music popup's mute hit target and removed the stray volume-slider
  dot. Volume updates keep using retained visuals and coalesced audio writes.
- Web apps installed from a browser (Edge, Chrome) get their own dock icon, as
  on the Windows taskbar, instead of joining the browser's. A pinned web app
  shows the name and icon of its shortcut, and clicking it switches to its open
  window instead of opening another.
- Spring-loaded drag and drop now opens app groups on hover and allows dropping
  files onto member apps. Hovering over an app with multiple windows cycles
  through them while dragging, with tactile icon bounces when a spring action
  triggers. Files can now be dropped onto pinned `.lnk` shortcuts as well as
  `.exe` targets.

### Español

- Los cambios del ahorro actualizan el estado de transparencia directamente
  desde Windows antes de redibujar el dock y la barra, con estado inicial y
  respaldo para sistemas anteriores. Sin sondeo en reposo.
- El contador del Dock tiene prioridad sobre el punto de atención, evitando
  insignias superpuestas. Se actualiza y se borra sin reconstruir el dock.
- Corregida la zona de clic de silencio en música y retirado el punto suelto
  del volumen. Se conservan los gráficos y las escrituras de audio agrupadas.
- Las aplicaciones web instaladas desde un navegador (Edge, Chrome) tienen su
  propio icono en el dock, como en la barra de Windows, en vez de unirse al del
  navegador. Una aplicación web anclada muestra el nombre y el icono de su acceso
  directo, y al hacer clic cambia a su ventana abierta en vez de abrir otra.
- El arrastre con apertura automática ahora despliega grupos de apps al mantener
  el cursor y permite soltar archivos en sus aplicaciones miembro. Mantener el
  arrastre sobre una app con varias ventanas alterna entre ellas, con un salto
  táctil en el icono al activarse. Ahora se pueden soltar archivos en accesos
  directos `.lnk` fijados además de ejecutables `.exe`.

## [0.1.19]

### English

- Added Dock settings for Start, Downloads and Recycle Bin visibility, optional File Explorer
  pinning, three spacing presets with previews, and three auto-hide animation
  speeds. Existing appearance and edge activation delays remain the defaults.
- Start, Downloads and Recycle Bin can also be hidden with "Remove from Dock"
  in their context menus. Settings refreshes their saved visibility on focus.
- Removed Nosih's moon shortcut from the menu bar. Do Not Disturb remains in
  Settings and clears queued banners while keeping unread badges.
- Fixed the dock appearing over full-screen playback controls (#33). Full-screen
  detection now follows owned controls, distinguishes custom-frame maximized
  windows, and reacts to in-place size changes without permanent polling.
- Added Z.ai GLM Coding Plan usage limits to the menu bar island. The 5-hour rolling quota and weekly plan limits appear alongside their reset times whenever OpenCode CLI is active with a configured Z.ai Coding Plan key.
- Fixed notification controls and stray conditional text appearing in Menu Bar
  settings. Each page now starts at the top when selected. Failed preference
  writes remain retryable and keep Settings open until changes can be saved.
- Fixed stale reset-confirmation timers; added Settings rendering and save-failure
  regression checks to CI without new runtime dependencies.
- Settings now shows the actual Windows state for automatic Search, Task View,
  Widgets and battery-percentage switches. Explicit choices remain authoritative;
  system values are display-only and refresh on focus without idle polling.
- Added macOS-style glass notification banners supporting configurable screen positions (top-right or bottom-right), auto-dismiss with hover pause, click-to-activate, and dismiss button or middle-click.
- Added red unread notification count badges on dock icons, cleared automatically when the app is focused or dismissed.
- Added dock icon attention bounce on shell alert events (`HSHELL_FLASH`), respecting Windows reduced-motion settings.
- Added Do Not Disturb (Focus) toggle in the menu bar to silence banners while preserving dock badges.
- Added local notification CLI and IPC: `nosih notify --title <TITLE> --body <BODY> [--app <APP>] [--icon <PATH>] [--duration <SECS>]` via lightweight zero-polling `WM_COPYDATA`.
- New Settings → Notifications section with toggles for banners, position, duration, badges, attention bounce, and Do Not Disturb.
- Hover previews on dock icons now display a close button ("✕") at the top-left
  corner on hover, allowing individual windows to close gracefully via WM_CLOSE
  without activating them.
- Middle-clicking any window preview thumbnail or minimized window in the dock
  now closes that window directly.
- Window previews now support single-window apps as well as multi-window apps,
  automatically re-layout when windows close, and close the popup when all
  windows have closed.
- New setting in Settings → Windows: "Show close button on previews" (on by default).
- Added macOS-style traffic light symbols (close ✕, minimize −, and zoom opposing diagonal expand triangles) to the Settings window controls, smoothly revealed upon hovering the buttons.
- Notification banners now trigger automatically when desktop applications (such as WhatsApp, Slack, Telegram, or Discord) request attention in the background, smoothly displaying macOS glass banners with click-to-activate.
- Suppressed redundant native Windows toast rectangular popups while Nosih is running, restoring Windows toast settings on exit.

### Español

- Añadidos ajustes del Dock para ocultar Inicio, Descargas y Papelera, dejar de mantener
  Explorador fijado, elegir tres espaciados con previsualización y tres velocidades
  de animación al ocultar. Se conservan el aspecto y las esperas actuales por defecto.
- Inicio, Descargas y Papelera también se pueden ocultar con «Quitar del Dock»
  en sus menús contextuales. Ajustes refleja su visibilidad guardada al recuperar el foco.
- Retirado el acceso de luna de la barra superior. No molestar se mantiene en
  Ajustes y descarta banners pendientes sin borrar las insignias.
- Corregido el dock que aparecía sobre los controles de reproducción en pantalla
  completa (#33). La detección contempla controles flotantes vinculados a su
  ventana, distingue ventanas maximizadas con marco propio y atiende cambios de
  tamaño sin sondeo permanente.
- Incorporado el uso del plan Z.ai GLM Coding Plan a la isla de la barra superior. El cupo móvil de 5 horas y el límite semanal aparecen junto con sus horas de reinicio cuando OpenCode CLI está en ejecución con una clave de Z.ai Coding Plan configurada.
- Corregidas las opciones de Notificaciones y el texto de una condición que
  aparecían en Barra superior. Cada sección comienza arriba al seleccionarla.
  Los cambios que fallen al guardarse se conservan para reintentar y Ajustes
  permanece abierto hasta poder guardarlos.
- Corregidos los temporizadores antiguos de confirmación al restablecer;
  añadidas pruebas de pantallas y fallos de guardado a CI sin nuevas dependencias.
- Ajustes muestra el estado real de Windows en los interruptores automáticos de
  Búsqueda, Vista de tareas, Widgets y porcentaje de batería. Las elecciones
  explícitas tienen prioridad; los valores del sistema solo se muestran y se
  releen al recuperar el foco, sin sondeo en reposo.
- Incorporadas alertas en banners de cristal estilo macOS con posición configurable (superior o inferior derecha), desaparición automática con pausa al pasar el cursor, clic para activar y botón de cierre o clic central para descartar.
- Incorporadas insignias rojas de contador de notificaciones pendientes en los iconos del dock, eliminadas automáticamente al enfocar la app o descartar sus avisos.
- Incorporada animación de rebote de atención en el dock cuando una app solicita atención (`HSHELL_FLASH`), respetando las preferencias de movimiento reducido de Windows.
- Incorporado conmutador de concentración / «No molestar» en la barra superior para silenciar avisos manteniendo las insignias en el dock.
- Incorporada interfaz local por línea de comandos e IPC: `nosih notify --title <TÍTULO> --body <CUERPO> [--app <APP>] [--icon <RUTA>] [--duration <SEGS>]` mediante `WM_COPYDATA` ligero y sin sondeos.
- Nueva sección Ajustes → Notificaciones con opciones de banners, posición, duración, insignias, rebote de atención y No molestar.
- Las previsualizaciones al pasar el cursor sobre los iconos del dock ahora muestran
  un botón de cerrar («✕») en la esquina superior izquierda al posarse sobre ellas,
  permitiendo cerrar ventanas individuales mediante WM_CLOSE sin activarlas.
- Hacer clic central en cualquier miniatura de previsualización o en ventanas
  minimizadas en el dock ahora cierra esa ventana directamente.
- Las previsualizaciones ahora admiten tanto apps de una sola ventana como multi-ventana,
  recalculando la disposición automáticamente al cerrarse ventanas y cerrando la ventana
  emergente cuando no quedan más ventanas abiertas.
- Nuevo ajuste en Ajustes → Ventanas: «Mostrar botón de cerrar en previsualizaciones»
  (activado por defecto).
- Incorporados símbolos estilo macOS en los botones de semáforo de la ventana de Ajustes (cerrar ✕, minimizar − y ampliar con triángulos opuestos en diagonal), mostrados con suavidad al posar el cursor.
- Las alertas en banners ahora se activan automáticamente cuando las aplicaciones de escritorio (como WhatsApp, Slack, Telegram o Discord) solicitan atención en segundo plano, mostrando banners de cristal estilo macOS que se activan al hacer clic.
- Supresión de los avisos rectangulares nativos de Windows mientras Nosih está en ejecución, restaurando el ajuste de Windows al salir.

## [0.1.18]

### English
- A folder or window picker left open no longer re-sends its unchanged visuals
  to the compositor every 50–60 ms: once open, it redraws only when the item
  under the pointer changes, so Windows does no composition work while it rests.
- Fixed island popup edge cases: AI cards stay within the display at high DPI,
  with clipped scrolling and reachable buttons. Volume drags cancel safely when
  the layout changes; audio-only updates reuse current system/provider data.
  Popups without room for their trigger close, repeated refresh clicks are
  disabled while busy, and failed media reads no longer retain the previous song.
  Scrolling reuses GPU visuals without extra idle polling or quota requests.
- Music now shows circular Windows media artwork in the island and its popup,
  with a bundled offline fallback. The popup adds system volume and mute controls;
  the thumb and percentage respond immediately, with coalesced audio writes and
  retained visuals. Audio events avoid unrelated system queries and full redraws
  for percentage-only changes. Release/cancellation does not steal app focus.
- Dock and island-style previews use the current monitor's wallpaper when available.
  Notch/bubble illustrations have clearer content and unobstructed selection marks. Nested
  options and sliders have fewer icons, and the GitHub star is filled yellow.
- Settings now illustrate Dock placement, island styles and Genie speeds, with
  native keyboard-selectable options, clearer icons and shorter descriptions.
  A permanent sidebar link opens Nosih on GitHub to explore and star the project.
- Settings → Dock → Position on screen: Left, Bottom or Right, as on macOS.
  Bottom stays the default. On a side the dock runs top to bottom below the
  menu bar, icons grow and show their names inward, menus, groups and window
  previews open beside the icon, the genie flies sideways into the dock, a
  hidden dock comes out of its side edge (on any display), and maximized
  windows end beside it. No top position: that edge belongs to the menu bar.
- Separated manual SignPath test builds from public releases and updater output.
  Signature verification now records Windows' detailed result and the file hash,
  checks the expected signer, and keeps strict integrity checks in both modes.
  Test-certificate trust is temporary and restricted to GitHub-hosted runners.
- Settings → Menu bar → Show what's playing (issue #28). The music half of
  the island could not be turned off; now it can, and with AI usage off too
  the menu bar shows no island at all. Off, Nosih also stops following media
  sessions. On by default, as before.
- Added "End task" beside "Quit" in a running app's context menu, including apps
  inside dock groups (issue #26). Force termination runs outside the UI thread
  against process handles captured for that menu. Explorer, critical/shared hosts
  and inaccessible processes are excluded; cancelling the menu changes nothing.
- Hardened "End task" against windows closed or added while its menu is open,
  including independent windows outside the dock's current desktop. All selected
  processes are checked before termination; an outdated selection is refused
  instead of ending a background process or partially applying the request.
- Settings → Windows → Genie speed (issue #27): Normal, Fast or Faster.
  Normal is the speed the genie always had (0.5 s, close to the macOS Dock)
  and stays the default; Fast takes 0.35 s and Faster 0.25 s, minimizing and
  restoring alike. The change applies to the next window at once.
- Added a persistent red attention badge for apps that request attention through
  Windows, including grouped apps and minimized-window thumbnails (issue #22,
  first stage). It clears on window activation/closure and follows Windows' badge
  preference live. Updates reuse visuals without rebuilding the dock, additional
  polling, notification-content access, or invented unread-message counts.
- Fixed (issue #25): an uninstalled app could stay in Recent Apps with no way
  to remove it. Recent apps that no longer exist now leave the dock on their
  own, as pinned ones already did, and clicking one that was just uninstalled
  removes it instead of failing. A recent app's menu also has "Remove from
  Dock", to dismiss it while recent apps stay on; it comes back the next time
  you open and close it.
- Fixed: a Logitech mouse on a receiver (reported with a G305 on its LIGHTSPEED
  receiver) did not show its battery in the menu bar, or showed it only for a
  moment. The mouse sleeps a few seconds after it stops moving and then does not
  answer, and every check dropped it. A device behind a receiver now keeps its
  last reading for 30 minutes while it sleeps, as Logitech's own tools do, and
  each request waits longer for a reply over the radio. Unplugging the
  receiver still removes it at once.
- OpenCode shows only measured Go quotas and their reset times, including the
  monthly window. Zen's local spending estimate and balance link are removed,
  together with the history queries and bundled SQLite dependency. A Zen-only
  connection shows no OpenCode indicator; missing quota data never becomes 0%.
- Fixed OpenCode Go account discovery: read its `opencode-go` credential, respect
  `XDG_DATA_HOME`, and fall back from an empty environment key to the saved login.
  Only saved Go keys or an explicit environment key are used for quota requests;
  saved Zen keys are not queried.
- Fixed a blocking "Ordinal 381 Not Found" system dialog when launching applications
  with Uninstall Tool (`utshellext.dll`) or other third-party shell extensions installed
  (issue #23). `nosih.exe` now embeds an application manifest declaring a dependency on
  `Microsoft.Windows.Common-Controls` 6.0.0.0 (along with PerMonitorV2 DPI awareness and
  Windows 10/11 compatibility), enabling shell extensions to resolve modern ComCtl32 exports
  such as `LoadIconWithScaleDown`.
- Process-level error mode now suppresses critical-error and file-not-found system message
  boxes (`SEM_FAILCRITICALERRORS | SEM_NOOPENFILEERRORBOX`) across the dock, the one-shot
  shell worker, and application launch, and `ShellExecuteExW` requests `SEE_MASK_FLAG_NO_UI`.
  Faulty or incompatible shell extensions no longer block or delay application opening;
  mandatory UAC elevation prompts remain visible.

### Español

- Una carpeta o un selector de ventanas abierto ya no reenvía al compositor sus
  visuales sin cambios cada 50–60 ms: una vez abierto, solo se redibuja cuando
  cambia el elemento bajo el puntero, así que Windows no compone nada mientras reposa.
- Corregidos casos límite de los popups: las tarjetas de IA caben en la pantalla
  con escala alta, con desplazamiento y botones limitados al área visible. El
  arrastre de volumen se cancela si cambia su posición; los cambios de audio
  reutilizan los datos del sistema y proveedores. Los popups sin espacio para
  su acceso se cierran, actualizar se desactiva mientras está ocupado y los
  errores de lectura multimedia no conservan la pista anterior. Desplazar la
  lista reutiliza visuales GPU, sin sondeo en reposo ni consultas de cuota extra.
- La isla y el popup de música muestran la carátula circular de Windows, con
  una imagen incluida como respaldo. El popup incorpora volumen del sistema y
  silencio: tirador y porcentaje responden al instante, reutilizando sus visuales
  y agrupando las escrituras de audio. Los cambios de porcentaje evitan consultas
  ajenas al audio y redibujar la barra. El arrastre no roba el foco.
- Las previews del Dock y de estilos de isla usan el fondo del monitor actual.
  Muesca y burbuja tienen contenido más claro y la marca de selección queda fuera
  de la imagen. Si el fondo no está disponible, mantienen el fondo ilustrado.
  Se quitan iconos de subopciones y sliders; la estrella de GitHub es amarilla.
- Ajustes incorpora vistas de la posición del Dock, los estilos de isla y la
  velocidad del genio, con selección por teclado, iconos y textos más breves.
  Un enlace fijo al pie del sidebar abre Nosih en GitHub para conocer y apoyar
  el proyecto con una estrella.
- Ajustes → Dock → Posición en la pantalla: Izquierda, Abajo o Derecha, como
  en macOS. Abajo sigue siendo la predeterminada. En un lateral el dock va de
  arriba abajo bajo la barra superior, los iconos crecen y muestran su nombre
  hacia dentro, los menús, grupos y vistas previas se abren junto al icono, el
  genio vuela de lado hacia el dock, el dock oculto sale por su borde lateral
  (en cualquier pantalla) y las ventanas maximizadas terminan a su lado. No hay
  posición arriba: ese borde es de la barra superior.
- Separadas las pruebas manuales de SignPath de las releases y el actualizador.
  La verificación registra el detalle de Windows y el hash, comprueba el firmante
  previsto y exige firmas válidas en ambos modos. La confianza del certificado
  de prueba es temporal y se limita a runners alojados de GitHub.
- Ajustes → Barra superior → Mostrar lo que se reproduce (issue #28). La parte
  de música de la isla no se podía apagar; ahora sí, y si también desactivas el
  uso de IA la barra superior no muestra ninguna isla. Apagado, Nosih deja
  además de seguir las sesiones multimedia. Activado por defecto, como antes.
- Añadido «Finalizar tarea» junto a «Salir» en el menú de una app abierta, también
  dentro de grupos del dock (issue #26). La finalización forzada se ejecuta fuera
  del hilo de interfaz y usa los procesos capturados para ese menú. Se excluyen
  Explorer, procesos críticos/hosts compartidos y procesos sin permisos; cancelar
  el menú no realiza ninguna acción.
- Reforzado «Finalizar tarea» si se cierran o aparecen ventanas mientras el menú
  sigue abierto, también fuera del escritorio que muestra el dock. Se comprueban
  todos los procesos antes de finalizar: una selección antigua se rechaza en vez
  de terminar un proceso en segundo plano o aplicar parcialmente la petición.
- Ajustes → Ventanas → Velocidad del genio (issue #27): Normal, Rápida o Muy
  rápida. Normal es la velocidad de siempre (0,5 s, cercana al Dock de macOS)
  y sigue siendo la predeterminada; Rápida dura 0,35 s y Muy rápida 0,25 s,
  tanto al minimizar como al restaurar. Se aplica al instante a la siguiente
  ventana.
- Añadido un badge rojo persistente para las apps que solicitan atención mediante
  Windows, también en grupos y miniaturas minimizadas (issue #22, primera etapa).
  Se quita al activar/cerrar la ventana y sigue en caliente la preferencia de
  badges de Windows. Se reutilizan los visuales sin reconstruir el dock, sin
  sondeos adicionales, sin leer mensajes y sin inventar contadores de no leídos.
- Corregido (issue #25): una app desinstalada podía quedarse en Apps recientes
  sin forma de quitarla. Las apps recientes que ya no existen salen solas del
  dock, como ya pasaba con las fijadas, y al hacer clic en una recién
  desinstalada se quita en vez de fallar. El menú de una app reciente tiene
  además «Quitar del Dock» para descartarla sin desactivar las recientes;
  vuelve la próxima vez que la abras y la cierres.
- Corregido: un ratón Logitech con receptor (reportado con un G305 y su receptor
  LIGHTSPEED) no mostraba su batería en la barra superior, o solo un momento. El
  ratón se duerme a los pocos segundos de no moverlo y entonces no responde, y
  cada comprobación lo quitaba. Un dispositivo tras receptor conserva ahora su
  última lectura 30 minutos mientras duerme, como las herramientas de Logitech, y
  cada petición espera más la respuesta por radio. Quitar el
  receptor lo sigue borrando al instante.
- Corregido un diálogo de error bloqueante ("No se encuentra el ordinal 381") al abrir
  aplicaciones con Uninstall Tool (`utshellext.dll`) u otras extensiones del shell de
  terceros instaladas (issue #23). `nosih.exe` ahora incrusta un manifiesto de aplicación
  que declara la dependencia de `Microsoft.Windows.Common-Controls` 6.0.0.0 (junto con
  reconocimiento DPI PerMonitorV2 y compatibilidad con Windows 10/11), permitiendo a las
  extensiones resolver funciones modernas de ComCtl32 como `LoadIconWithScaleDown`.
- El modo de error del proceso ahora suprime los cuadros de diálogo de errores críticos
  del sistema (`SEM_FAILCRITICALERRORS | SEM_NOOPENFILEERRORBOX`) en el dock, el proceso
  auxiliar del shell y el lanzador, y `ShellExecuteExW` usa `SEE_MASK_FLAG_NO_UI`. Las
  extensiones del shell defectuosas o incompatibles ya no bloquean ni retrasan la apertura
  de aplicaciones; las solicitudes de elevación UAC se mantienen visibles.
- WinGet manifests now use `/S` for both unattended modes and declare the
  Visual C++ x64 and WebView2 runtime dependencies. This addresses first-install
  UI and missing-runtime risks; Microsoft installation revalidation is pending.

### Español

- Corregida la detección de cuentas OpenCode Go: se lee la credencial `opencode-go`,
  se respeta `XDG_DATA_HOME` y una variable de clave vacía permite usar la sesión
  guardada. Las consultas usan únicamente la clave guardada de Go o una clave
  explícita de entorno; no se consulta con claves guardadas de Zen.
- OpenCode muestra solo cuotas medidas de Go y sus reinicios, incluido el mes.
  Se retiran el gasto local de Zen y el enlace al saldo, las consultas al historial
  y la dependencia SQLite. Una conexión solo Zen no muestra el indicador de
  OpenCode; la ausencia de cuotas nunca se convierte en 0 %.
- Los manifiestos de WinGet usan `/S` en ambos modos desatendidos y declaran
  las dependencias de Visual C++ x64 y WebView2. Se corrigen riesgos de interfaz
  en la primera instalación y de runtimes ausentes; falta revalidar con Microsoft.

## [0.1.17]

### English

- Added Google Search Console ownership verification to the website.
- Automated WinGet manifest generation and submission for the first package and
  future stable releases, with manual retries, native validation, final installer
  checksum checks, duplicate detection and explicit missing-token failures.
- Added bounded graphics-device recovery and recovery of an existing dock when
  Nosih is opened again. The watchdog closes the old instance before restarting;
  a blocked recovery close has an eight-second deadline. Repeated device loss
  within the first minute after recovery stops automatic restarts and restores
  the Windows taskbar. Graphics health reuses the existing watchdog heartbeat.
- An error updating the top bar no longer prevents the dock from refreshing or
  returning after full-screen mode.
- Added read-only USB adapters for SteelSeries Nova 5/5X and more Nova 7/7P
  variants, Logitech G PRO X 2 (Centurion), selected G PRO/G533 HID++ variants,
  HyperX Cloud Alpha Wireless and Kingston Cloud II Wireless. Voltage-only and
  discrete readings remain unknown; HP/DTS Cloud II revisions with a reported
  connection-query side effect are excluded. Physical validation is pending.
- Added Logitech Lightspeed and selected Nano receiver IDs, including the G305's
  C53F, and HID++ Battery Status 1000 reads. Short and long report collections are
  now queried together once per receiver. The 60-second interval and opt-in
  setting remain; physical validation of the expanded coverage is pending.
- Fixed three Clippy violations in the accessory diagnostic's serialization tests.
  Tag releases now run the full CI checks before building and signing the installer.

### Español

- Añadida la verificación de propiedad de Google Search Console en la web.
- Automatizada la generación y el envío de manifiestos de WinGet para la primera
  publicación y las próximas releases estables, con reintentos manuales, validación
  nativa, comprobación del hash final, detección de duplicados y error si falta el token.
- Añadida recuperación limitada ante pérdida del dispositivo gráfico y al abrir
  Nosih de nuevo cuando ya existe una instancia. El guardián cierra la anterior
  antes de reiniciarla; si el cierre de recuperación se bloquea, tiene un plazo de
  ocho segundos. Un nuevo fallo gráfico durante el primer minuto tras recuperar
  detiene los reinicios automáticos y devuelve la barra de Windows. La revisión
  gráfica aprovecha la comprobación de salud que ya hacía el guardián.
- Un error al actualizar la barra superior ya no impide actualizar el dock ni
  hacerlo reaparecer al salir de pantalla completa.
- Añadidos adaptadores USB de lectura para SteelSeries Nova 5/5X y más variantes
  Nova 7/7P, Logitech G PRO X 2 (Centurion), variantes G PRO/G533 HID++, HyperX
  Cloud Alpha Wireless y Cloud II Wireless Kingston. Voltajes y niveles discretos
  siguen sin porcentaje; se excluyen revisiones HP/DTS de Cloud II cuya consulta
  de conexión tiene un efecto secundario documentado. Falta validación física.
- Añadidos receptores Logitech Lightspeed y Nano identificados, incluido C53F del
  G305, y lectura de batería HID++ 1000. Las colecciones de informes cortos y largos
  se consultan juntas una sola vez por receptor. Se mantienen los 60 segundos y
  la activación opcional; la ampliación queda pendiente de validación física.
- Corregidos tres errores de Clippy en las pruebas de serialización del diagnóstico
  de accesorios. Las releases por etiqueta ejecutan ahora todos los controles de CI
  antes de compilar y firmar el instalador.

## [0.1.16]

### English

- Initial automatic USB battery adapters for allowlisted Logitech Unifying/Bolt
  receivers and SteelSeries Nova 7 variants. Queries run in disposable processes,
  pause with the hidden menu bar and use a 60-second interval with error backoff.
  Approximate levels stay unknown. Physical validation is still pending; radio
  link changes may take until the next query to appear.
- Optional mouse/headset battery indicators in Settings → Menu bar, off by default.
  Enabling them detects connected accessories automatically; missing battery data
  displays a dash. Disabling them stops the isolated reader and all accessory
  queries. The landing's Settings demo includes the switch and inline indicators.
  Hardware compatibility still needs validation.
- The menu bar now follows Windows' Battery percentage setting, showing the
  charge next to the battery icon when enabled. Preference changes apply
  without restarting Nosih, through registry notifications rather than polling.
- Fixed icons remaining over the desktop after dragging an app out of a dock
  group. Drag tracking now recovers missed pointer messages without taking
  focus, stops on release or Escape, and rejects late clicks after cancellation.
- Grouping now requires dropping over the target icon vertically as well as
  horizontally. Recovered or collapsed groups no longer duplicate pinned apps
  in recent apps.
- Fixed (issue #15): the Recycle Bin could keep its full icon after being
  emptied, even after restarting Nosih. Right after a change, Explorer may
  still draw the previous state, and that icon was cached for the day under
  the other state. The empty and full icons now come from where Windows names
  them (`DefaultIcon`, including an icon you chose in Desktop icon settings),
  and wrongly cached ones are replaced on their own.

### Español

- Adaptadores USB iniciales para receptores Logitech Unifying/Bolt y variantes
  SteelSeries Nova 7 identificadas. Consultas en procesos desechables, pausadas
  con la barra oculta, con intervalo de 60 segundos y espera tras errores.
  Los niveles aproximados siguen sin porcentaje. Falta validación física; los
  cambios del enlace radio pueden tardar hasta la consulta siguiente.
- Indicadores opcionales de batería del ratón y auriculares en Ajustes → Barra
  superior, desactivados por defecto. Al activarlos se detectan los accesorios
  conectados automáticamente; un guion indica batería no disponible. Desactivarlos
  termina el lector aislado y todas las consultas de accesorios. La demo de Ajustes
  de la landing incluye el interruptor y los indicadores en la barra.
  La compatibilidad con hardware real queda pendiente de validar.
- La barra superior respeta la opción Porcentaje de batería de Windows y
  muestra la carga junto al icono cuando está activada. Los cambios se aplican
  sin reiniciar Nosih, mediante notificaciones del registro, sin sondeo.
- Corregido el icono que quedaba flotando sobre el escritorio al sacar una app
  de un grupo del dock. El arrastre recupera mensajes del puntero perdidos sin
  tomar el foco, termina al soltar o pulsar Escape y descarta clics tardíos tras
  una cancelación.
- Agrupar requiere soltar sobre el icono también en vertical. Recuperar o
  reducir un grupo ya no duplica apps fijadas en las apps recientes.
- Corregido (issue #15): la Papelera podía seguir con el icono de llena tras
  vaciarla, incluso al reiniciar Nosih. Justo después de un cambio, Explorer
  puede dibujar aún el estado anterior, y ese icono se guardaba en caché todo
  el día bajo el otro estado. Los iconos de vacía y llena se leen ahora de
  donde Windows los define (`DefaultIcon`, incluido un icono elegido en la
  configuración de iconos del escritorio), y los guardados mal se reemplazan solos.

## [0.1.15]

### English

- Dragging apps out of the dock or a group now completes or cancels reliably
  even when a non-activating window misses mouse input outside its bounds.
  Press Escape to cancel a drag.
- Dock groups: drag a pinned app onto the middle of another one (a blue frame
  shows it) to put both in a group that takes a single place in the dock;
  drop more apps on the group to add them. A group shows its first apps'
  icons on a framed tile, and lists their names on hover. Clicking it fans
  its apps out above it (a grid when there are many); drag one out of the
  fan to take it out of the group, into the dock where it is dropped.
  Right-click a group to ungroup it.
- Shortcuts folder: choose a folder in Settings → Dock and it shows at the
  right of the dock. Clicking it opens its contents as a menu, as a Windows 10
  taskbar toolbar did: folders first, each one a submenu, then files, with
  Explorer's names and icons; choosing an item opens it. Right-click it to
  open the folder, change it or remove it.
- French: the dock, Settings and the installer are now also in French. Choose
  it in Settings → System, or leave Automatic to follow Windows. A language
  Nosih does not know in the settings file now reads as Automatic instead of
  making the file invalid.
- Mascot showcase: the website now previews the crab's reactions to AI limits,
  charging and energy saver, and the hero crab moves between the Notes window
  and dock. Choose a moment to play its scene or let the preview run; reduced
  motion is respected.
- Apps that showed a blank page instead of their icon now show it, as on the
  taskbar: LibreOffice (its windows belong to `soffice.bin`; the dock now
  shows LibreOffice Calc, Writer… with their names and icons), and programs
  whose icon the Windows shell cannot draw although it is valid (one made
  only of PNG images, as some Electron tools write). Icons are read again
  once after updating.

### Español

- Arrastrar apps fuera del dock o de un grupo ahora termina o se cancela de
  forma fiable aunque una ventana que no toma el foco pierda los eventos del
  mouse fuera de sus límites. Pulsa Esc para cancelar el arrastre.
- Grupos en el dock: arrastra una app fijada al centro de otra (un marco azul
  lo indica) para reunirlas en un grupo que ocupa un solo lugar en el dock;
  suelta más apps sobre el grupo para añadirlas. Un grupo muestra los iconos
  de sus primeras apps en un mosaico enmarcado, y sus nombres al pasar el
  puntero. Al hacer clic se despliegan en abanico encima (en cuadrícula si son
  muchas); arrastra una fuera del abanico para sacarla del grupo, al lugar del
  dock donde la sueltes. Clic derecho en un grupo para desagruparlo.
- Carpeta de accesos directos: elige una carpeta en Ajustes → Dock y aparece
  a la derecha del dock. Al hacer clic, su contenido se abre como un menú,
  como una barra de herramientas de la barra de tareas de Windows 10: primero
  las carpetas, cada una un submenú, luego los archivos, con los nombres e
  iconos del Explorador; al elegir un elemento se abre. Clic derecho para
  abrir la carpeta, cambiarla o quitarla.
- Francés: el dock, Ajustes y el instalador también están en francés.
  Elígelo en Ajustes → Sistema, o deja Automático para seguir a Windows. Un
  idioma que Nosih no conoce en el archivo de ajustes ahora se lee como
  Automático en lugar de invalidar el archivo.
- Vista previa de la mascota: la web muestra las reacciones del cangrejo ante
  los límites de IA, la carga y el ahorro de energía; además, en la portada se
  mueve entre la ventana de Notas y el dock. Elige un momento para ver su escena
  o deja que la vista previa avance; se respeta la opción de reducir movimiento.
- Las apps que mostraban una página en blanco en lugar de su icono ahora lo
  muestran, como en la barra de tareas: LibreOffice (sus ventanas pertenecen
  a `soffice.bin`; el dock ahora muestra LibreOffice Calc, Writer… con sus
  nombres e iconos) y los programas cuyo icono el shell de Windows no sabe
  dibujar aunque sea válido (uno hecho solo de imágenes PNG, como escriben
  algunas herramientas de Electron). Los iconos se vuelven a leer una vez
  tras actualizar.

## [0.1.14]

### English

- Clicking an app whose minimized window is still reported as foreground now
  restores it on the first click, including with one app open and minimized
  previews disabled (#12). File drag-hover uses the same native-state check.

- Automatic startup retries up to three times after a failed launch. Existing
  enabled tasks receive the recovery policy without enabling disabled tasks or
  changing the installation they launch. Settings checks the real registration.
- If Explorer has not created its taskbar yet, Nosih retries taking it over
  without blocking the interface or saving a false original taskbar setting.
- Startup failures before graphics initialization are now recorded in the log.
- Dock window previews keep their positions when switching or minimizing
  windows. Newly discovered windows append; closed windows are removed. Normal
  icon activation continues to use the most recently used window.

### Español

- Pulsar el icono de una app minimizada que Windows aún indica como activa
  ahora la restaura al primer clic, incluso con una sola app abierta y las
  miniaturas de minimizadas desactivadas (#12). El arrastre sobre un icono
  utiliza la misma comprobación del estado real de la ventana.

- El inicio automático reintenta hasta tres veces si falla el arranque. Las
  tareas activadas reciben esta recuperación sin reactivar tareas deshabilitadas
  ni cambiar la instalación que ejecutan. Ajustes consulta el registro real.
- Si Explorer todavía no ha creado su barra, Nosih vuelve a intentar ocultarla
  sin bloquear la interfaz ni guardar un estado original incorrecto.
- El registro también recoge fallos anteriores a la inicialización gráfica.

- Las previsualizaciones del dock conservan su posición al cambiar de ventana
  o minimizarla. Las ventanas nuevas se añaden al final y las cerradas se retiran.
  El clic normal del icono sigue recuperando la ventana usada más recientemente.

## [0.1.13]

### English

- The menu bar's keyboard indicator opens a themed layout menu with the
  current keyboard marked and a link to Windows language settings. Changes
  are requested for the original typing window instead of simulating Win+Space.
- Changing Nosih's language refreshes menu bar text even when its data has
  not changed. Menu bar controls retain the standard arrow cursor.
- Menus return focus after a selection without overriding another app the
  user switched to while dismissing them. Auto-hide keeps the menu bar open
  while its keyboard selector is in use.

- With auto-hide enabled, rest the pointer at a display's bottom edge to
  bring the Dock to that display (#9, #11).
- Menu bar redraws no longer cancel a pending top-edge reveal (#10).
- Screen-edge detection reads the cursor on a dedicated timer thread while
  auto-hide is active, including when the cursor is already at the edge.
- Auto-hidden menu bars verify the cursor position before closing on
  mouse-leave notifications, with a pointer check only while the bar is revealed.
- The menu bar reveals after a shorter edge dwell and slides/fades in and
  out using the Windows compositor. Returning during its exit reverses the
  transition; Windows' disabled animation setting is respected.
- Show/hide transitions wait for compositor completion and keep hidden content
  transparent, preventing cached-frame flashes. Auto-hide also applies before
  the menu bar's first frame at startup.
- Closing a dock context menu resets hover and resumes auto-hide when the
  pointer is outside the dock. File drags keep the dock open.
- Dock and Settings writes are serialized. Changing a preference preserves
  recent pin changes and the dock's latest auto-hide setting. Closing
  Settings waits for pending edits to be saved.
- Taking over the Windows taskbar after resume or an Explorer restart runs
  in the background, as it does at startup.

### Español

- El indicador de teclado de la barra abre un menú con el estilo de Nosih,
  marca el teclado activo y permite abrir los ajustes de idioma de Windows.
  El cambio se solicita a la ventana donde se escribía, sin simular Win+Espacio.
- Cambiar el idioma de Nosih actualiza los textos de la barra aunque no
  cambien sus datos. Los controles de la barra mantienen el cursor de flecha.
- Los menús devuelven el foco tras una selección sin imponerse a otra app
  elegida al cerrarlos. La barra permanece visible mientras se usa su
  selector de teclado, incluso con ocultación automática.

- Con la ocultación automática activada, deja el puntero en el borde inferior
  de una pantalla para mostrar el Dock allí (#9, #11).
- Actualizar la barra superior ya no cancela su aparición al dejar el puntero
  en el borde (#10).
- La detección del borde consulta el puntero desde un hilo con temporizador
  mientras está activa la ocultación automática, incluso si ya está en el borde.
- Los avisos de salida de la barra superior verifican la posición real del
  puntero antes de cerrarla, con una comprobación solo mientras está desplegada.
- La barra aparece con una espera menor en el borde y transiciones de
  deslizamiento y opacidad mediante el compositor de Windows. Volver durante
  la salida la invierte; respeta la desactivación de animaciones de Windows.
- La salida espera a que termine realmente la animación del compositor y el
  contenido permanece transparente al ocultarse, evitando destellos de fotogramas
  anteriores. La auto-ocultación se aplica desde el primer fotograma al iniciar.
- Al cerrar un menú contextual fuera del dock se restablece el puntero y
  vuelve a funcionar la ocultación automática. Al arrastrar archivos, el
  dock permanece abierto.
- El dock y Ajustes guardan por turnos. Cambiar una preferencia conserva
  los pines y el estado de ocultación más recientes. Ajustes guarda los
  cambios pendientes antes de cerrarse.
- Recuperar la barra de tareas tras reanudar Windows o reiniciar el
  Explorador se ejecuta en segundo plano, igual que en el inicio.

## [0.1.12]

### English

- Unplugging the charger now plays a short mascot reaction above the dock,
  including when unplugging also enables energy saver. It replaces an ongoing
  charging reaction and temporarily reveals an auto-hidden dock.
- The first interactive installation opens Nosih Settings when it completes.

### Español

- Al desconectar el cargador, la mascota también aparece sobre el dock,
  incluso si se activa el ahorro de energía a la vez. Sustituye la reacción
  de carga si todavía está en curso y muestra temporalmente el dock oculto.
- La primera instalación interactiva abre Ajustes de Nosih al terminar.

## [0.1.11]

### English

- Removed the manual option to import apps pinned to the Windows taskbar from
  the dock's context menu. Changes to Windows taskbar pins still sync
  automatically.
- Clicking an app while a shell flyout has focus accepts native pointer
  activation before opening it. Minimized windows are restored even if the
  subsequent foreground request is refused. Hidden stale windows are skipped.
- Dock button clicks resolve the current pointer position instead of relying
  on a potentially coalesced mouse-move event.
- Minimization flights keep their capture when Windows focuses the next app.
  Deferred restores still cancel when the user switches to a different app.
- The mascot respects the Windows animation preference and stops when
  disabled, entering full screen or changing display geometry. Initial
  power notifications establish a baseline. Power acts start after the
  dock refresh, and hover reactions preserve the act's exit animation.
  Failed creation no longer reveals an auto-hidden dock or consumes the
  charging cooldown. AI quota cues ignore failed or older measurements
  and comparisons between different quota windows.
- Energy-saver notifications now update the dock and menu bar directly from
  the state sent by Windows. Standard and high-savings modes both disable
  glass, and turning the saver off restores the transparency preference.
- Nosih uses an interactive logon task to avoid Windows' discretionary delay
  of `Run` entries. Windows still controls when the process can start.
  Registration uses the Task Scheduler API with normal priority, battery
  support and a `Run` fallback. Enabled registrations migrate only for the
  matching installation. Migration and Settings changes are serialized,
  and task-removal failures are reported.
- Logon tasks use the account's SID, which survives account renames.
  Changes to autostart run off the Settings UI thread.
- Fixed: after turning the PC on, the dock and the menu bar could stay hidden
  and the Windows taskbar showed instead. Right after signing in, the window
  in front is often a hidden shell window the size of the screen (Start,
  Search or the text input host), and the dock took it for a full-screen app.
  Hidden and cloaked windows no longer count as full screen.
- Diagnostics are written off the UI thread through a bounded queue.
  `%LOCALAPPDATA%\Nosih\nosih.log` and its rotated backup each hold up to
  256 KB. The log records errors, full-screen transitions and initialization
  time measured from entering the dock initialization routine.

### Español

- Se eliminó del menú contextual del dock la opción manual para añadir las
  apps fijadas en la barra de tareas de Windows. Los cambios en esos pines
  siguen sincronizándose automáticamente.
- Al pulsar una app mientras un panel del sistema conserva el foco, el dock
  acepta la activación nativa del clic antes de abrirla. Las ventanas
  minimizadas se restauran aunque se rechace la petición posterior de foco.
  Las ventanas ocultas que hayan quedado en la lista se descartan.
- Los clics del dock consultan la posición actual del puntero, evitando usar
  el icono de un movimiento anterior cuando Windows agrupa eventos del ratón.
- El efecto de minimización conserva la captura cuando Windows enfoca la
  siguiente app. Las restauraciones aplazadas se cancelan si el usuario
  cambia a otra aplicación.
- La mascota respeta la opción de animaciones de Windows y se cancela al
  desactivarla, entrar en pantalla completa o cambiar la geometría de pantalla.
  Los avisos iniciales de energía establecen el estado de referencia.
  Sus escenas comienzan después de actualizar el dock y las reacciones al
  puntero conservan la animación de salida. Un fallo al crearla ya no revela
  el dock oculto ni consume la espera entre avisos del cargador. Los avisos
  de IA descartan lecturas fallidas, antiguas o de ventanas de cuota distintas.
- Las notificaciones de ahorro de energía actualizan el dock y la barra
  superior con el estado enviado por Windows. Tanto el modo estándar como
  el ahorro máximo desactivan el cristal. Al desactivarlos, se recupera la
  preferencia de transparencia del sistema.
- Nosih usa una tarea de inicio de sesión interactiva para evitar el retraso
  que Windows puede aplicar a las entradas `Run`. Windows sigue decidiendo
  cuándo puede arrancar el proceso. La tarea se registra con la API del
  Programador, prioridad normal, soporte para batería y respaldo mediante
  `Run`. Se migran los registros activos de la misma instalación. Los cambios
  desde Ajustes y la migración se ejecutan de forma serializada y los errores
  al eliminar tareas se comunican.
- Las tareas usan el SID de la cuenta, que se conserva al cambiar su nombre.
  Cambiar el inicio automático se ejecuta fuera del hilo de Ajustes.
- Corregido: al encender el PC, el dock y la barra superior podían quedarse
  ocultos y se veía la barra de tareas de Windows. Justo al iniciar sesión, la
  ventana al frente suele ser una ventana oculta del shell del tamaño de la
  pantalla (Inicio, Búsqueda o el host de entrada de texto), y el dock la
  tomaba por una app a pantalla completa. Las ventanas ocultas ya no cuentan.
- El registro de diagnóstico se escribe fuera del hilo de la interfaz mediante
  una cola acotada. `%LOCALAPPDATA%\Nosih\nosih.log` y su copia anterior
  ocupan hasta 256 KB cada uno. Guarda errores, cambios de pantalla completa
  y el tiempo medido desde que comienza la inicialización del dock.

## [0.1.10]

### English

- Right-clicking an empty area of the dock now opens Task Manager or Taskbar
  settings.
- Apps with multiple windows have a dedicated preview picker with more
  reliable hover and foreground handling.
- The Genie effect follows Windows' "Animation effects" setting, which
  "Adjust for best performance" also turns off, and did so silently: the
  switch in Settings stayed on and nothing happened. Settings now says when
  Windows animations are off and opens the right Windows page to turn them on.
- A pixel crab comes out for moments worth it, and only then: it hops with a
  heart under an AI provider whose limit just reset, trembles with a drop
  past 90 %, falls asleep at 100 %, scurries along the dock when the charger
  is plugged in, and dozes off on it when energy saver starts. Hover over it
  and it reacts. An auto-hidden dock comes up for the moment, as the macOS
  Dock does. While no crab is out it costs nothing; its acts run in the system
  compositor. It can be turned off in Settings → Dock.
- Fixed (issue #8): apps pinned from a Windows shortcut with arguments, such
  as `wscript.exe "script.vbs"`, a browser profile or a web app, opened the
  program alone. They now open through their shortcut, exactly as from
  Explorer or the taskbar, and show the shortcut's name and icon. A shortcut
  dropped on the dock is kept in `%APPDATA%\Nosih\Pins`, so the pin survives
  deleting the original; shortcuts to scripts or documents can be pinned too.
- Dragging a file over the dock reads a dropped shortcut once, not on every
  mouse move.

### Español

- Al hacer clic derecho en un espacio vacío del dock, ahora puedes abrir el
  Administrador de tareas o la configuración de la barra de tareas.
- Las apps con varias ventanas tienen un selector de vistas previas propio,
  con un manejo más fiable del puntero y de la ventana en primer plano.
- El efecto genio sigue el ajuste «Efectos de animación» de Windows, que también
  apaga «Ajustar para obtener el mejor rendimiento», y lo hacía en silencio: el
  interruptor de Ajustes seguía activo y no pasaba nada. Ahora Ajustes avisa
  cuando las animaciones de Windows están desactivadas y abre la página de
  Windows correcta para activarlas.
- Un cangrejo pixelado aparece en los momentos que lo merecen, y solo
  entonces: salta con un corazón bajo el proveedor de IA cuyo límite se acaba
  de reiniciar, tiembla con una gota al pasar del 90 %, se duerme al llegar al
  100 %, cruza el dock corriendo al enchufar el cargador y se echa una siesta
  encima al activarse el ahorro de energía. Si pasas el puntero por encima,
  reacciona. El dock oculto sube durante ese momento, como hace el Dock de
  macOS. Mientras no hay cangrejo no consume nada; sus escenas las ejecuta el
  compositor del sistema. Se desactiva en Ajustes → Dock.
- Corregido (issue #8): las apps fijadas desde un acceso directo de Windows con
  argumentos, como `wscript.exe "script.vbs"`, un perfil de navegador o una
  app web, abrían solo el programa. Ahora se abren a través de su acceso
  directo, igual que desde el Explorador o la barra de tareas, y muestran su
  nombre e icono. Un acceso directo soltado en el dock se guarda en
  `%APPDATA%\Nosih\Pins`, así que la app fijada sigue funcionando aunque se
  borre el original; también se pueden fijar accesos a scripts o documentos.
- Al arrastrar un archivo sobre el dock, un acceso directo se lee una sola vez,
  no en cada movimiento del ratón.

## [0.1.9]

### English

- Fixed: bringing back a minimized app from the dock could leave its window
  behind the one in front, as if it had closed again (Warp, for one). Since
  0.1.8 the dock restored the window without taking the focus first, and
  Windows then places it behind the active window.
- GitHub Copilot usage was removed from the menu bar island. Its sign-in is
  only stored encrypted, and decrypting another app's secrets is what made
  antivirus programs flag Nosih; Nosih no longer reads the Windows
  Credential Manager either.
- Fixed: some antivirus programs flagged Nosih 0.1.6–0.1.8 as a trojan and
  deleted it, often right after updating. Two things looked like malware: to
  show GitHub Copilot and Cursor usage, Nosih decrypted VS Code's stored
  secrets and read Cursor's database (what password stealers do), and since
  0.1.7 it set itself up to start with Windows by running `schtasks.exe`
  hidden. Nosih no longer does either: it starts from the usual `Run`
  registry entry (an existing startup task is moved there once), Copilot
  usage comes only from the Copilot CLI sign-in or `GH_TOKEN`/`GITHUB_TOKEN`,
  and Cursor usage was removed. The dock is also 28% smaller.
- Fixed: after choosing an item in the dock's menu (Nosih Settings, for
  one), the dock could stay magnified and stop reacting to the pointer.
- Alt+Tab: the hidden dock shows only for Alt+Tab, no longer for Task View,
  whose virtual desktops it covered at the bottom of the screen; and it no
  longer flashes for a moment after cancelling Alt+Tab with Esc.
- Alt+Tab: moving the mouse or clicking an empty spot no longer stops the
  dock's magnifier from following the selection, and closing a window from
  the switcher keeps it on the right app. Only the arrow keys, which move the
  selection in ways Nosih cannot see, make it step aside.
- Automatic updates: once a day, a couple of minutes after you turn on the
  computer (or every 24 hours if it stays on), Nosih installs new versions by
  itself; the dock restarts in a moment. Never while an app is full screen. It can be turned off in Settings → System → "Install
  updates automatically". The dock itself still never uses the network.
- Windows 10: the menu bar no longer shows a Widgets button, which Windows
  10 does not have; it opened Windows Ink instead.
- Glass follows energy saver: turning it on or off (by hand or at low battery)
  switches the dock, the menu bar and the dock menu between glass and solid
  at once.

### Español

- Corregido: al recuperar una app minimizada desde el dock, su ventana podía
  quedar detrás de la que estaba delante, como si se hubiera cerrado otra vez
  (Warp, por ejemplo). Desde la 0.1.8 el dock restauraba la ventana sin tomar
  antes el foco, y Windows la coloca entonces detrás de la ventana activa.
- Se quitó el uso de GitHub Copilot de la isla de la barra superior. Su sesión
  solo se guarda cifrada, y descifrar los secretos de otra app es lo que hacía
  que los antivirus marcaran Nosih; Nosih tampoco lee ya el Administrador de
  credenciales de Windows.
- Corregido: algunos antivirus marcaban Nosih 0.1.6–0.1.8 como troyano y lo
  borraban, a menudo justo al actualizar. Dos cosas parecían malware: para
  mostrar el uso de GitHub Copilot y Cursor, Nosih descifraba los secretos
  guardados de VS Code y leía la base de datos de Cursor (lo que hacen los
  ladrones de contraseñas), y desde la 0.1.7 se configuraba para iniciar con
  Windows ejecutando `schtasks.exe` oculto. Nosih ya no hace ninguna de las
  dos: arranca desde la entrada habitual `Run` del registro (una tarea de
  inicio existente se traslada ahí una vez), el uso de Copilot sale solo de la
  sesión de Copilot CLI o de `GH_TOKEN`/`GITHUB_TOKEN`, y se quitó el uso de
  Cursor. Además, el dock ocupa un 28 % menos.
- Corregido: tras elegir una opción del menú del dock (por ejemplo, Ajustes
  de Nosih), el dock podía quedarse ampliado y dejar de responder al puntero.
- Alt+Tab: el dock oculto aparece solo con Alt+Tab, ya no con la Vista de
  tareas, cuyos escritorios virtuales tapaba abajo; y ya no parpadea un
  momento después de cancelar Alt+Tab con Esc.
- Alt+Tab: mover el ratón o hacer clic en un hueco ya no impide que la lupa
  del dock siga la selección, y cerrar una ventana desde el selector la
  mantiene en la app correcta. Solo las flechas, que mueven la selección de
  formas que Nosih no ve, hacen que se aparte.
- Actualizaciones automáticas: una vez al día, un par de minutos después de
  encender el ordenador (o cada 24 horas si sigue encendido), Nosih instala
  las versiones nuevas por sí solo; el dock se reinicia en un momento. Nunca
  con una app a pantalla completa. Se puede desactivar en Ajustes →
  Sistema → «Instalar actualizaciones automáticamente». El dock en sí sigue
  sin usar la red.
- Windows 10: la barra superior ya no muestra un botón de Widgets, que Windows
  10 no tiene; abría Windows Ink en su lugar.
- El cristal sigue al ahorro de energía: al activarlo o desactivarlo (a mano o
  con batería baja), el dock, la barra superior y el menú del dock cambian
  entre cristal y sólido al momento.

## [0.1.8]

### English

- Opening apps and shell file operations run in temporary helper processes so
  slow shell handlers and native progress dialogs keep the dock responsive.
  Transfers run in order; ordinary launch helpers time out after 30 seconds,
  while file operations and administrator launches keep their native user controls.
- Recycle Bin notifications share one drive scan at a time, preserving changes
  that arrive during a scan without starting overlapping disk queries.
- Shell icons and app names load in the background through a bounded queue,
  keeping the dock responsive while the shell is slow. Repeated requests share
  the same load, and unused pixel buffers are released.
- Minimize animations prepare restore images off the UI thread and reuse
  thumbnail dimensions between window events. Batch minimization starts one
  capture; a stalled capture no longer keeps an overlay and frame loop alive.
- Drag and drop into apps: hold a file (or text, or an image) over an open
  app in the dock and its window comes to the front, so you can drop it there,
  as with the Windows taskbar. Passing over icons does not switch windows.
- Fixed: with "Automatically hide and show the menu bar" on, the bar covered the
  minimize, maximize and close buttons of maximized windows and took clicks
  meant for browser tabs at the top edge. The hidden bar no longer keeps any
  window on screen; it appears after the pointer rests briefly on the top
  edge, and never over a window's title-bar buttons.
- The top-right corner of the menu bar no longer shows the desktop: throwing
  the pointer there to close a window minimized everything. "Show Desktop"
  remains in the dock menu.
- Fixed: clicking the menu bar's "^" again left the hidden-icons popup open
  and moved the pointer to its first icon. The second click now closes it.
- The dock's menu looks like a macOS menu: frosted glass in the light or dark
  theme, compact rows and the selection in your Windows accent color. It
  works fully from the keyboard; with a screen reader or high contrast on,
  the native Windows menu is used instead.
- An auto-hidden dock shows while Alt+Tab (or Task View) is open, so the
  open apps are in view, and hides again when you let go. As you press Tab,
  the dock magnifies the app of the selected window. Windows does not report
  the selection, so it follows Tab and Shift+Tab; after arrow keys or the
  mouse in the switcher, the magnifier steps aside rather than guess.
- The menu bar no longer shows a location icon. Widgets keeps location in use
  for the weather all the time, and Windows shows no icon for that either.
- Windows 10: apps pinned to the taskbar from the Microsoft Store (Store, Mail
  and others) now come to the dock. Versions before 0.1.7 could not import
  them, and updating did not add them afterwards.

### Español

- Abrir apps y ejecutar operaciones de archivos del shell se hace en procesos
  auxiliares temporales para mantener la respuesta del dock mientras el shell
  tarda o muestra el progreso. Las transferencias se ejecutan en orden; las
  aperturas normales tienen un límite de 30 segundos, mientras las operaciones
  de archivos y las aperturas como administrador conservan sus controles nativos.
- Los avisos de Papelera comparten una sola lectura de discos a la vez y
  conservan los cambios que lleguen durante esa lectura, sin consultas solapadas.
- Los iconos y nombres del shell se cargan en segundo plano con una cola
  limitada para que el dock responda mientras el shell tarda. Las solicitudes
  repetidas comparten la misma carga y se liberan los píxeles que ya no se usan.
- Las animaciones de minimizar preparan la imagen de restauración fuera del
  hilo de interfaz y reutilizan las dimensiones de las miniaturas entre avisos
  de ventanas. Al minimizar en lote se inicia una captura; una captura atascada
  ya no mantiene una vista previa y un ciclo de fotogramas indefinidos.
- Arrastrar y soltar en las apps: mantén un archivo (o texto, o una imagen)
  sobre una app abierta del dock y su ventana pasa al frente para que lo
  sueltes ahí, como con la barra de tareas de Windows. Pasar por encima de los
  iconos no cambia de ventana.
- Corregido: con «Ocultar y mostrar la barra superior automáticamente», la barra tapaba
  los botones de minimizar, maximizar y cerrar de las ventanas maximizadas y
  se quedaba con los clics en las pestañas del navegador del borde superior.
  La barra oculta ya no deja ninguna ventana en pantalla: aparece cuando el
  puntero se detiene un momento en el borde superior, y nunca encima de los
  botones de una ventana.
- La esquina superior derecha de la barra ya no muestra el escritorio: lanzar
  el puntero ahí para cerrar una ventana minimizaba todo. «Mostrar escritorio»
  sigue en el menú del dock.
- Corregido: volver a pulsar «^» en la barra superior dejaba abierto el
  panel de iconos ocultos y movía el puntero a su primer icono. Ahora el
  segundo clic lo cierra.
- El menú del dock se ve como un menú de macOS: cristal esmerilado en tema
  claro u oscuro, filas compactas y la selección en el color de énfasis de
  Windows. Funciona por completo con el teclado; con un lector de pantalla o
  alto contraste activos se usa el menú nativo de Windows.
- El dock oculto aparece mientras Alt+Tab (o la Vista de tareas) está abierto,
  para ver las apps abiertas, y se vuelve a ocultar al soltar. Al pulsar Tab,
  el dock amplía la app de la ventana seleccionada. Windows no informa de la
  selección, así que sigue Tab y Mayús+Tab; si se usan las flechas o el ratón
  en el selector, la lupa se aparta en vez de adivinar.
- La barra superior ya no muestra el icono de ubicación. Widgets mantiene la
  ubicación en uso todo el tiempo para el clima, y Windows tampoco muestra un
  icono por eso.
- Windows 10: las apps de Microsoft Store fijadas en la barra de tareas
  (Store, Correo y otras) ahora llegan al dock. Las versiones anteriores a la
  0.1.7 no podían importarlas, y al actualizar no se añadían después.

## [0.1.7]

### English

- Faster startup: Nosih now uses a Windows sign-in task instead of the Run
  key, and keeps the taskbar hidden when Windows shuts down or restarts.
- Taskbar pins stay in sync with the dock, including changes made while Nosih
  is closed. Imported pins keep their order and include Store apps; removed
  apps no longer remain as broken dock icons.
- The dock menu can import apps pinned to the Windows taskbar. Unchecking
  "Keep in Dock" also unpins the app from the taskbar.
- Middle-click or Shift-click opens another app window; Ctrl+Shift-click or
  the dock menu opens an app as administrator. The menu bar's right edge and
  the dock menu can show the desktop.
- The menu bar shows the active input language and privacy indicators for
  microphone, camera, and location use. Its clock follows time and time-zone
  changes, and switching virtual desktops updates the dock immediately.
- Genie: maximized and snapped windows now fly using the area they occupy on
  screen, without jumping to a different size.

### Español

- Inicio más rápido: Nosih ahora usa una tarea de Windows al iniciar sesión
  en lugar de la clave Run y mantiene oculta la barra de tareas al apagar o
  reiniciar Windows.
- Las apps fijadas en la barra de tareas se sincronizan con el dock, incluso
  si el cambio ocurre mientras Nosih está cerrado. Las apps importadas
  conservan su orden e incluyen las de la Store; las desinstaladas ya no quedan
  como iconos rotos.
- El menú del dock permite importar las apps fijadas en la barra de tareas.
  Desmarcar «Mantener en el Dock» también desancla la app de la barra.
- El clic central o Mayús+clic abre otra ventana; Ctrl+Mayús+clic o el menú del
  dock abre una app como administrador. El borde derecho de la barra superior
  y el menú del dock permiten mostrar el escritorio.
- La barra superior muestra el idioma de entrada activo e indicadores cuando
  se usan el micrófono, la cámara o la ubicación. El reloj sigue los cambios
  de hora y zona horaria, y al cambiar de escritorio virtual el dock se
  actualiza de inmediato.
- Genio: las ventanas maximizadas o acopladas vuelan usando el área que ocupan
  en pantalla, sin saltar a otro tamaño.

## [0.1.6]

### English

- Windows 10 support (version 1903 or later). The dock used to close right
  after starting there. On Windows 10 the dock and menu bar are solid instead
  of glass, since it has no backdrop blur for desktop apps.
- The menu bar's "^" button opens Windows' hidden notification-area icons.
- A curved menu bar island shows usage for Claude, Codex, GitHub Copilot,
  Cursor, OpenCode Go, and Gemini or Antigravity while they are running. It can
  be turned off in Settings → Menu bar.
- Settings has a clearer layout with separate Dock, Windows, Menu bar and
  System sections, and its window can be resized or maximized.
- Minimized windows now fly into their app's dock icon by default. Live
  thumbnails on the right side of the dock remain available as an option.
- The active-app click option was removed: clicking the app in front minimizes
  it, as on the Windows taskbar.
- The dock's Nosih icon is sharper, and the Start button uses the Windows
  logo.
- Settings can turn off automatic update checks. The website adds privacy and
  code-signing policy pages.

### Español

- Soporte para Windows 10 (versión 1903 o posterior). Antes el dock se cerraba
  nada más arrancar ahí. En Windows 10 el dock y la barra superior son sólidos
  en lugar de cristal, porque no ofrece desenfoque de fondo a las apps de
  escritorio.
- El botón «^» de la barra superior abre los iconos ocultos del área de
  notificación de Windows.
- Una isla curva en la barra superior muestra el uso de Claude, Codex, GitHub
  Copilot, Cursor, OpenCode Go y Gemini o Antigravity mientras están abiertos.
  Se puede desactivar en Ajustes → Barra superior.
- Ajustes tiene secciones separadas para Dock, Ventanas, Barra superior y
  Sistema; su ventana se puede redimensionar o maximizar.
- Por defecto, las ventanas minimizadas vuelan hacia el icono de su app en el
  dock. Las miniaturas en vivo a la derecha siguen disponibles como opción.
- Se quitó la opción de clic en la app activa: al hacer clic en la app que está
  delante, se minimiza, como en la barra de Windows.
- El icono de Nosih se ve más nítido en el dock y el botón Inicio usa el logo
  de Windows.
- Ajustes permite desactivar la búsqueda automática de actualizaciones. La web
  incorpora páginas de privacidad y firma de código.

## [0.1.5]

### English

- The dock appears sooner at startup (about 170 ms instead of 260-450 ms).
- Lower idle CPU: windows changing their title (terminals, browsers) no longer
  make the dock re-read every window.
- Genie starts sooner after minimizing: the screen copy no longer waits extra
  frames.
- Fixed: the dock and menu bar could stay black after energy saver; without
  transparency they are now solid gray, as macOS draws them with "Reduce
  transparency".
- Fixed: the menu bar could miss an update and showed no app name until the
  first window switch.
- Menu bar: the battery shows a charging bolt.
- Settings: options that depend on another fold away under it.

### Español

- El dock aparece antes al arrancar (unos 170 ms en lugar de 260-450 ms).
- Menos CPU en reposo: las ventanas que cambian de título (terminales,
  navegadores) ya no hacen que el dock relea todas las ventanas.
- El genio empieza antes al minimizar: la copia de pantalla ya no espera
  fotogramas de más.
- Corregido: el dock y la barra superior podían quedarse negros tras el ahorro
  de energía; sin transparencia ahora son grises sólidos, como macOS con
  «Reducir transparencia».
- Corregido: la barra superior podía perder una actualización y no mostraba el
  nombre de la app hasta el primer cambio de ventana.
- Barra superior: la batería muestra un rayo mientras carga.
- Ajustes: las opciones que dependen de otra se pliegan bajo ella.

## [0.1.4]

### English

- Maintenance release: align Genie default documentation and tests with the
  shipped setting.
- Website: add an Open Graph preview image and document the observed idle
  memory reading.

### Español

- Versión de mantenimiento: alinear la documentación y las pruebas del efecto
  Genio con el ajuste publicado.
- Web: añadir una imagen de vista previa Open Graph y documentar la lectura
  observada de memoria en reposo.

## [0.1.2]

### English

- Updates are announced in Settings: a banner above every section with
  "Update now", "Later" and "What's new", download progress, and a badge on
  System while an update waits.
- Smoother Genie effect: the system compositor now runs the whole animation
  at the display's refresh rate, and it no longer skips its first frames.
- Genie: no more flash when minimizing, and a window restored with Alt+Tab or
  from the app itself animates again the next time it is minimized.
- Fixed: the Dock section of Settings could not scroll to its last options.
- Fixed: opening Settings twice opened two windows; it now brings the open one
  to the front.
- Fixed: with an app running as administrator in front (Task Manager, an admin
  terminal, an installer), the dock's Start button and the menu bar buttons did
  nothing, because Windows drops simulated keys aimed at such apps.

### Español

- Las actualizaciones se anuncian en Ajustes: un aviso encima de cada sección
  con «Actualizar ahora», «Más tarde» y «Novedades», progreso de la descarga y
  una insignia en Sistema mientras haya una pendiente.
- Efecto genio más fluido: el compositor del sistema ejecuta toda la animación
  a la frecuencia del monitor y ya no se salta sus primeros fotogramas.
- Genio: ya no hay destello al minimizar, y una ventana restaurada con Alt+Tab o
  desde la propia app vuelve a animarse la siguiente vez que se minimiza.
- Corregido: la sección Dock de Ajustes no dejaba bajar hasta las últimas opciones.
- Corregido: abrir Ajustes dos veces abría dos ventanas; ahora trae al frente la abierta.
- Corregido: con una app de administrador delante (Administrador de tareas, una
  terminal de administrador, un instalador), el botón Inicio del dock y los botones
  de la barra superior no hacían nada, porque Windows descarta las teclas simuladas
  dirigidas a esas apps.

## [0.1.1]

### English

- Menu bar: network (Wi-Fi strength, Ethernet or offline) and volume, updated
  as they change. Search, Task View and Widgets buttons appear when they are on
  in your Windows taskbar settings.
- Clicking the app that is already in front minimizes it, as the Windows
  taskbar does; click again to bring it back. Settings can switch this to
  "do nothing", as on a Mac.
- Apps opened from the dock come to the front reliably instead of sometimes
  opening behind other windows.
- Minimized apps that run as administrator, such as Task Manager, can be brought
  back from the dock.
- Settings: restore defaults, and options to show open-app indicators, show
  recent apps and animate opening apps.

### Español

- Barra superior: red (intensidad del Wi-Fi, Ethernet o sin conexión) y volumen,
  actualizados al momento. Los botones de búsqueda, Vista de tareas y Widgets
  aparecen si los tienes activados en la barra de tareas de Windows.
- Clic en la app que ya está delante la minimiza, como la barra de Windows; otro
  clic la vuelve a traer. En Ajustes se puede cambiar a «no hacer nada», como en Mac.
- Las apps abiertas desde el dock aparecen delante siempre, en vez de abrirse a
  veces detrás de otras ventanas.
- Las apps minimizadas que se ejecutan como administrador, como el Administrador
  de tareas, se pueden recuperar desde el dock.
- Ajustes: restablecer valores y opciones para mostrar los indicadores de apps
  abiertas, mostrar las apps recientes y animar al abrir apps.

## [0.1.0]

### English

- First release: glass dock with magnification, live minimized-window
  thumbnails, optional Genie effect, menu bar, Settings app in English and
  Spanish, one-step installer and in-app updates.

### Español

- Primera versión: dock de cristal con ampliación, miniaturas en vivo de las
  ventanas minimizadas, efecto genio opcional, barra superior, app de Ajustes en
  español e inglés, instalador de un paso y actualizaciones desde la app.
