# Batería de accesorios en la barra superior

La opción **Ajustes → Barra superior → Mostrar batería de accesorios** está
desactivada por defecto. Al activarla, la detección es automática mientras exista
la barra superior. Un ratón y unos auriculares conectados muestran su icono y
porcentaje junto al idioma y la batería del equipo. No hay popup ni botón Bluetooth vacío.
Sin accesorios conectados no se reserva espacio. Los indicadores son pasivos y
conservan el cursor de flecha, la tipografía, colores y escala de la barra.

## Datos y compatibilidad

Bluetooth ya emparejado y adaptadores USB incluidos en la lista de esta entrega,
siempre con confirmación de conexión del accesorio, no solo del receptor. Se elige un dispositivo por categoría en orden estable, con prioridad
para la salida actual de audio. La batería viene de metadatos de Windows unidos
por Container ID; la propiedad de compatibilidad no es un contrato universal.

Sin nivel único se muestra «—»; 0 % sigue siendo un valor válido. Niveles
contradictorios no se promedian y nombres iguales no fusionan dispositivos.
Un fallo o lectura incompleta oculta los indicadores hasta recuperar datos;
no se presentan porcentajes anteriores como actuales.

Para USB solo se abre la colección de fabricante del VID/PID autorizado, después
de comprobar identidad, usage y longitudes de informe. No hay conexiones GATT,
búsqueda de vecinos ni consultas a receptores desconocidos. No se conoce la carga del estuche, valores por
auricular o el instante de medición física. `accessoryBattery` guarda la opción;
si falta en un archivo existente, vale `false`. La selección antigua
`accessoryPins` se ignora: los dispositivos se eligen automáticamente.

Todavía no hay hardware inalámbrico para contrastar niveles y cambios reales.
No se anuncian marcas o modelos compatibles. Windows 10 requiere prueba propia.

## Recursos y recuperación

Con la opción apagada no se crea el monitor, su proceso auxiliar, suscripciones
ni consultas USB. Apagarla libera el monitor existente y retira los indicadores.
Reanudación, cambios de audio y reintentos no pueden reactivarlo. Solo queda la
preferencia en memoria; las mediciones del lector de abajo corresponden a la
opción activada, no al estado inicial de la app.

El hilo gráfico solo compara dos lecturas pequeñas: cambios de nombre o de
dispositivos no representados no reconstruyen la barra. El mismo ejecutable con
`--accessories-worker` aloja los proveedores WinRT en MTA, fuera del hilo gráfico.
Tres watchers filtrados, cola no bloqueante de 256 eventos y agrupación de ráfagas
durante 120 ms evitan barridos por fotograma. Bluetooth sigue usando eventos;
los receptores USB tienen la política de consulta descrita más abajo.

El padre limita cada operación a cinco segundos y cada mensaje a 64 KiB/32
dispositivos. Quitar la barra termina el auxiliar sin `join` en la UI. Un Job
Object con `KILL_ON_JOB_CLOSE` también lo termina si el dock falla. El lector
revoca su espera antes de liberar el contexto. Reanudación y cambio de salida
de audio reconstruyen las suscripciones; tras errores, el reloj ya existente
permite reintentar como máximo una vez por minuto, sin temporizador nuevo.

## Adaptadores USB iniciales · 11c

Son implementaciones en validación, todavía sin compatibilidad física certificada.

| Receptor/familia | VID:PID | Colección y lectura |
|---|---|---|
| Logitech Unifying | 046D:C52B, 046D:C532 | FF00:0002, informes HID++ largos de 20 bytes. Slots 1–6; ping con eco y consulta del tipo. Porcentaje solo si 1004 anuncia estado de carga real. |
| Logitech Bolt | 046D:C548 | Misma política. Un dispositivo HID++ 1.0 o sin batería exacta no recibe un porcentaje inventado. |
| Logitech Lightspeed | 046D:C539, C53A, C53D, C53F, C541, C545, C547, C54D | Un slot por receptor; incluye la ruta documentada del G305/G304 en C53F. Descubrimiento HID++ 2.0+ de batería 1004 y respaldo 1000. |
| Logitech Nano | 046D:C52E, C52F, C531, C534, C537 | Solo periféricos HID++ 2.0+ con las colecciones correspondientes. C534 permite dos slots; los demás, uno. No implica soporte de variantes HID++ 1.0. |
| SteelSeries Nova 7 original | 1038:2202 | Interfaz 3, FFC0:0001; informe B0 con estado del enlace. Sus niveles discretos permanecen «—», no 25/50/75 % ficticios. |
| SteelSeries Nova 7 actualizado / Gen 2 | 1038:22A1, 1038:227E | Misma colección, porcentaje directo 0–100 y estado de conexión. |
| SteelSeries Nova 5 / 5X | 1038:2232, 1038:2253 | FFC0:0001, interfaz 3. Estado B0 con disposición propia: conexión en byte 1 y porcentaje en byte 3 del cuerpo. |
| SteelSeries Nova 7/7X/7P y ediciones, nivel discreto | 1038:2206, 220A, 223A, 227A, 22A4 | Misma lectura que 2202. Nivel 0–4 mostrado como «—». |
| SteelSeries Nova 7/7X/7P actualizado, porcentaje | 1038:2258, 229E, 22AD, 22A9, 22A5, 22A7, 2298 | Misma lectura que 22A1/227E; porcentaje sin conversión de niveles. |
| Logitech G PRO / PRO X / variantes X2 HID++, G533 | 046D:0AA7, 0AAA, 0ABA, 0AFB, 0AFC, 0A66 | FF43:0202, informes de 20 bytes e índice directo FF. Descubrir SoC 1004/1000; ADC 1F20 solo confirma una lectura de voltaje y deja el porcentaje en «—». |
| Logitech G PRO X 2 Lightspeed, Centurion | 046D:0AF7 | FFA0:0001, interfaz 3, informes 64/64 e ID 51. Consultas Root/Bridge y Battery SoC 0104. Sin inicialización ni fallback a paquetes legacy. |
| HyperX Cloud Alpha Wireless | 03F0:098D | Colección de fabricante con informe 21 y salida 31/32 bytes. Confirmación de enlace antes de batería; durante carga, «—». |
| HyperX Cloud II Wireless, Kingston no DTS | 0951:1718 | Colección de fabricante con informe 06 y salida de 62 bytes. Consultas 01 (enlace) y 02 (batería), respuestas 0B/BB con eco del comando. |

No se amplía por marca o similitud del nombre. El filtro de Windows limita la
enumeración a estas colecciones; antes de escribir una solicitud de lectura se
verifican de nuevo VID/PID, descriptor y tamaños con un handle sin acceso a datos.
Solo después se abre lectura/escritura compartida de esa colección. Los paquetes
son consultas de estado, nunca cambios de ajustes, luces, emparejamiento o firmware.
No se usan curvas de voltaje ni se fuerza un valor a entrar en 0–100.

### Corrección del G305 y transporte Logitech

El receptor C53F no estaba en el descubrimiento original; además, el G305 publica
su batería mediante **Battery Status 1000**, no Unified Battery 1004. La consulta
ahora descubre 1000 si 1004 no proporciona porcentaje. Se conserva el porcentaje
que devuelve el dispositivo, incluidos sus posibles saltos de nivel; no se
interpolan pasos ni voltajes. En 1000, cero significa desconocido; el cero válido
de 1004 con capacidad SoC sigue mostrándose como 0 %.

Segunda corrección (2026-11-20, el G305 seguía sin verse). El protocolo era
correcto: Solaar muestra el G305 (C53F, PID inalámbrico 4074) con
`BATTERY STATUS {1000}` y «Battery: 90%, discharging» (issues #850 y #792). Lo que
fallaba era el ciclo: el G305 duerme a los pocos segundos sin moverse y entonces no
contesta por radio. Cada consulta reemplazaba la lista, así que el ratón
desaparecía en casi todas las consultas (y con cualquier error). Ahora se conserva
la lectura de los slots de receptor durante 30 minutos y la petición HID++ espera
400 ms en lugar de 150 ms.

Windows separa los informes HID++ de 7 y 20 bytes. Se descubren ambas colecciones
FF00:0001/0002, se agrupan por instancia USB e interfaz y se consultan en un solo
proceso. Antes de escribir se validan VID/PID, usage y tamaños de cada handle.
Ambas lecturas overlapped están preparadas antes de enviar la solicitud; una
espera de kernel recibe la respuesta corta o larga. Slot, función, software ID y
eco del ping descartan mensajes ajenos. No hay hilo residente por colección.

La agrupación no mezcla dos receptores iguales. Añadir o retirar una colección
invalida consultas anteriores; avisos repetidos no adelantan el próximo sondeo.
La ampliación inicial tuvo 16 receptores Logitech identificados y tres variantes
Nova. La ampliación de auriculares suma 23 identidades: el registro actual tiene
42 VID/PID explícitos. Sigue pendiente la validación física por modelo.

### Alcance de la ampliación de auriculares

Los adaptadores propios están en `accessories/usb/headsets.rs` y
`headsets/centurion.rs`; las pruebas usan trazas simuladas. Para Centurion se
descubren únicamente Bridge y Battery SoC: cuatro consultas en el caso habitual,
sin recorrer todas las funciones. Se comprueban longitudes externas/internas,
software ID, feature y estado de enlace. Los ACK y las respuestas ajenas no son
una medición; los eventos de desconexión retiran el accesorio.

En HyperX se conservan los porcentajes del dispositivo, incluido 0 %. Los Cloud
II **HP/DTS** 03F0:0696/018B no se incluyen: su lector de referencia documenta que
la consulta de conexión puede restablecer estados y quitar el silencio del micro.
Esto no se extrapola al protocolo Kingston 0951:1718, que es distinto. No se
envían consultas de mute, sidetone, inicialización, emparejamiento o firmware.

El descubrimiento sigue filtrado por VID/PID y colección. Solo las dos revisiones
HyperX sin usage fijo publicado usan un filtro de página de fabricante por su
VID/PID exacto. Antes de abrir acceso de datos se validan descriptor, tamaños e ID
de informe de salida; la colección equivocada no recibe comandos. Esta validación
se aplica también a los receptores existentes. No se añaden hilos residentes ni
temporizadores. La lectura se prepara antes de escribir para no perder una
respuesta rápida, y se cancela al vencer el presupuesto o cerrar la consulta.

Los Logitech que solo exponen voltaje no tendrán un porcentaje calculado con una
curva prestada. Que otro lector muestre un estimado no convierte ese valor en SoC.
G435, G733, otras revisiones de los mismos nombres y otros IDs no se anuncian
como compatibles por compartir marca. El usuario todavía no dispone de hardware
inalámbrico para comprobar estas rutas: hacen falta pruebas reales de enlace,
carga, suspensión y convivencia con G HUB, GG y NGENUITY.

### Política de consumo y aislamiento

- Sin interfaces autorizadas: ningún proceso de consulta USB ni hilo del pool.
- Hasta ocho receptores, dos colecciones por receptor y una consulta en curso,
  cola de un trabajo y generaciones
  para descartar respuestas de un receptor retirado/reconectado. Quitar el último
  receptor libera el pool al acabar su trabajo acotado.
- Una consulta inicial al detectar la interfaz y, como respaldo, cada 60 segundos
  mientras la barra esté visible. El receptor puede responder que el accesorio está
  apagado: también se limita a 60 segundos ese intento de descubrirlo.
- Pausa mientras la barra esté auto-oculta o cubierta. Dos eventos de kernel comunican
  visibilidad; no hay un timer de UI ni escrituras bloqueantes a una tubería.
- Cada consulta corre en otro `nosih.exe --accessory-usb-query`, con Job Object y
  plazo de dos segundos. HID usa I/O overlapped, 150 ms por intercambio con auriculares,
  400 ms por petición HID++ a través de receptor (la radio despierta antes al ratón;
  el monitor de referencia usa 400 ms en su ping) y 1.250 ms para la secuencia. Si un controlador no cancela, solo se termina ese hijo;
  el coordinador Bluetooth no espera la consulta.
- Ante errores: la espera crece desde dos hasta quince minutos. Un auricular directo
  que no responde se retira; un dispositivo tras receptor (slot 1–6) que no responde,
  porque duerme o la consulta falló, conserva su última lectura 30 minutos, como el
  monitor de referencia (`graceSec = 1800`). Quitar el receptor la borra al instante.
  Los cambios de metadatos no reinician el plazo. Reanudación o cambio de salida de
  audio reconstruyen el lector; no se registra la identidad del periférico en logs.

El intervalo de 60 segundos sustituye al inicial de cinco minutos. En operación
normal, por interfaz visible, pasa de unas 12 a unas 60 consultas por hora
(aproximadamente 48 adicionales). El coste de CPU acumulado de las consultas
crece con su frecuencia; no implica que el consumo total de Nosih se multiplique
por cinco. Se mantienen el mismo pool, una consulta simultánea y el mismo límite
de memoria de cada proceso temporal.

Sin receptor identificado o con la barra oculta no hay consultas USB adicionales.
El coste real con un receptor sigue pendiente de medición: incluye creación del
proceso, trabajo de sus controladores y posible actividad del periférico. El
timeout de dos segundos es tiempo de pared; no equivale a dos segundos de CPU.

**Límite funcional:** desenchufar el USB se detecta por eventos. Encender/apagar el
accesorio dejando el receptor puesto puede reflejarse hasta la siguiente consulta
(60 segundos visibles; después de errores, hasta quince minutos). No se anuncia detección
instantánea del enlace radio. Las notificaciones HID permanentes se evaluarán con
hardware para evitar lectores que interfieran con software del fabricante.

## Verificación

```powershell
cargo test -p nosih -p nosih-shell -p nosih-config --locked
cargo clippy -p nosih -p nosih-shell -p nosih-config --all-targets --locked -- -D warnings
cargo build -p nosih --locked
$env:NOSIH_ACCESSORY_TEST_EXE = 'C:\Users\USUARIO\.cargo-target\nosih\debug\nosih.exe'
cargo test -p nosih --locked monitor_repeated_shutdown_and_timeout_are_bounded -- --ignored --test-threads=1
cargo test -p nosih --locked usb_process_deadline_terminates_a_stalled_provider -- --ignored --test-threads=1
cargo test -p nosih --locked accessory_indicators_render_without_controls_or_popups -- --ignored --test-threads=1
cargo test -p nosih --locked accessory_opt_out_blocks_startup_resume_and_retry -- --ignored --test-threads=1
cargo test -p nosih --locked long_requests_receive_short_or_long_replies_and_cancel_the_other_read -- --test-threads=1
```

Ajustar la variable al ejecutable de ese equipo. Las pruebas cubren protocolo por
trazas simuladas, comandos exclusivamente de consulta, 0/100, niveles discretos,
tramas ajenas/truncadas, interfaces desconocidas, desconexión, respuestas tardías,
backoff, señales de visibilidad y limpieza del supervisor. La prueba nativa de
UI conserva los 16 casos de escala/tema/transparencia, sin controles ni popups.

Pendiente de hardware: comparar niveles con Windows/fabricante, radio apagada,
reconexión, suspensión, dos receptores iguales y coexistencia con Options+/G HUB/GG.
No se dispone de esos dispositivos; Windows 10 requiere prueba independiente.

### Resultado local · 2026-11-19

133 pruebas normales correctas, Clippy sin advertencias y compilación optimizada
correcta. Pasaron también las pruebas nativas del supervisor, del hijo USB detenido
y de los indicadores. El hijo detenido fue terminado en unos 120 ms con un plazo
de prueba de 100 ms; no se accedió a hardware en esa simulación.

Lector optimizado con el canal de visibilidad real, tres segundos de calentamiento
y dos muestras consecutivas de cinco segundos, sin periféricos inalámbricos:

| Estado de la barra | CPU registrada | Memoria privada | Handles | Hijos USB observados |
|---|---|---|---|---|
| Visible | 0 ms | 4,027 MiB | 244 | 0 |
| Oculta | 0 ms | 4,027 MiB | 244 | 0 |

Es una muestra corta del auxiliar, con resolución limitada; no demuestra consumo
absoluto cero, coste con un receptor conectado, ausencia de fugas prolongadas ni
el consumo total del dock. La enumeración terminó correctamente con lista vacía.

Tras reducir el intervalo a 60 segundos: 133 pruebas normales, Clippy y compilación
optimizada correctos. Una nueva muestra del lector con su señal de visibilidad,
sin periféricos y tras tres segundos de calentamiento, registró 0 ms de CPU en
cinco segundos, 4,098 MiB privados y 244 handles. Es una comprobación del reposo;
no cuantifica el incremento con un receptor ni permite atribuir variaciones pequeñas
de memoria entre ejecuciones al intervalo. Ese incremento sigue pendiente de hardware.

Tras añadir el control opcional: 134 pruebas normales y Clippy correctos. La
regresión nativa comprueba el arranque apagado, activar/desactivar y que ni la
reanudación ni un reintento vencido creen de nuevo un monitor desactivado. Ajustes
y la demo Astro usan el mismo valor inicial apagado.
Compilación optimizada de dock y Ajustes correcta. La ventana real de Ajustes
guardó `true` y `false` mediante su interruptor usando un archivo temporal
independiente. La demo se comprobó en inglés/español y anchos de 390/1440 px;
`astro check` y build estático correctos. Estas pruebas no sustituyen la
validación física pendiente.

Tras ampliar Logitech: `cargo clippy --workspace --all-targets -- -D warnings`
y `cargo test --workspace --quiet` correctos (229 pruebas, 25 omitidas de forma
explícita). La prueba nativa de tuberías recibe respuestas de 7/20 bytes después
de una solicitud larga y cancela la otra lectura pendiente. Las comprobaciones
manuales del supervisor y del proceso USB bloqueado también pasaron; este último
terminó en unos 120 ms con un plazo de prueba de 100 ms.

Compilación optimizada correcta. Una ejecución aislada del lector durante cinco
segundos llegó a `Ready`, con lista vacía y sin errores de descubrimiento USB:
el filtro ampliado arranca en este Windows. No había periféricos compatibles;
esto no valida los porcentajes del G305 físico ni el coste con un receptor.
La reversión de esta ampliación se limita a `accessories/usb/{protocol,io,mod}.rs`,
la dependencia de pruebas de Pipes en `apps/dock/Cargo.toml` y sus notas; no requiere
revertir preferencias, indicadores, Bluetooth ni los cambios independientes de CI.

### Verificación de la ampliación de auriculares · 2026-11-19

`cargo clippy --workspace --all-targets -- -D warnings` y las 239 pruebas del
workspace pasaron. Las nuevas trazas prueban cada secuencia completa, incluyendo
comandos exclusivamente de consulta, enlace apagado/emparejando, carga, 0/100 %,
niveles inválidos y mensajes ajenos. Centurion verifica ACK, software ID, índices
de features, longitudes anidadas y desconexión durante una consulta.

La prueba nativa del transporte pasó con tuberías locales: prepara la lectura,
rellena la salida al tamaño del descriptor y cancela una espera vencida. También
pasaron las comprobaciones nativas de cierre del supervisor, timeout USB y
desactivación de la función. El proceso USB simulado se terminó en unos 120 ms;
no se consultó ningún periférico en esas pruebas.

El watcher USB completó la enumeración del filtro ampliado en 0,71 s, sin abrir
handles HID de datos. El lector optimizado llegó a `Ready` sin errores durante
una muestra aislada de cinco segundos, con lista vacía. Son verificaciones de
integración en este Windows, no de compatibilidad física ni coste con hardware.
La compilación optimizada del dock también terminó correctamente.

Para ejecutar la prueba de filtro (solo metadatos):

```powershell
cargo test -p nosih --locked accessory_usb_discovery_enumerates_allowlist -- --ignored --test-threads=1
```

Reversión de este bloque: adaptadores `headsets.rs` y `headsets/`, sus perfiles y
ruta de consulta en `protocol.rs`/`io.rs`/`mod.rs`, validación de informes del
descriptor y la prueba de descubrimiento en `worker.rs`. No requiere retirar el
soporte previo de ratones, preferencias, indicadores o los arreglos de CI.

## Referencias de protocolo

Implementación propia en Rust, sin incorporar código ni binarios de otros lectores.
Se contrastaron los hechos del protocolo en estas fuentes primarias:

- [HID++ y formato de paquetes de Logitech](https://github.com/Logitech/cpg-docs/tree/master/hidpp20).
- [Identificadores de receptores en Solaar](https://github.com/pwr-Solaar/Solaar/blob/master/lib/logitech_receiver/base_usb.py).
- [Funciones y receptor del G305 documentados con hardware](https://github.com/pwr-Solaar/Solaar/issues/850).
- [Lectura Battery Status 1000 en Solaar](https://github.com/pwr-Solaar/Solaar/blob/master/lib/logitech_receiver/hidpp20.py).
- [Capacidad de porcentaje en HID++ 1004, controlador Linux](https://github.com/torvalds/linux/blob/master/drivers/hid/hid-logitech-hidpp.c).
- [Identidad y estados Nova 7 documentados por su lector](https://github.com/Sapd/HeadsetControl/blob/master/lib/devices/steelseries_arctis_nova_7.hpp).
- [Nova 5/5X y su disposición del estado](https://github.com/Sapd/HeadsetControl/blob/master/lib/devices/steelseries_arctis_nova_5.hpp).
- [Variantes Nova 7P](https://github.com/Sapd/HeadsetControl/blob/master/lib/devices/steelseries_arctis_nova_7p.hpp).
- [G PRO X 2 y Centurion](https://github.com/Sapd/HeadsetControl/blob/master/lib/devices/logitech_gpro_x2_lightspeed.hpp).
- [G PRO HID++ y lectura de voltaje](https://github.com/Sapd/HeadsetControl/blob/master/lib/devices/logitech_gpro.hpp).
- [Cloud Alpha Wireless](https://github.com/Sapd/HeadsetControl/blob/master/lib/devices/hyperx_cloud_alpha_wireless.hpp).
- [Protocolo Kingston Cloud II](https://github.com/LennardKittner/HyperHeadset/blob/main/old_cloud_2_doc/Headset_protocol.md).
- [Efecto de la consulta de conexión Cloud II DTS](https://github.com/LennardKittner/HyperHeadset/blob/main/src/devices/cloud_ii_wireless_dts.rs).
- [Obtención de informes HID en Windows](https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/obtaining-hid-reports-by-user-mode-applications).
