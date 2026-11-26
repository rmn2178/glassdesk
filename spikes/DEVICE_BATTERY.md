# Diagnóstico de batería de accesorios — entrega 11a

Prototipo propio en Rust para decidir qué datos puede consumir la barra superior.
Se ejecuta aparte: no arranca Nosih, no modifica su configuración y no incorpora
lectores ni dependencias residentes al dock. La integración visual corresponde a 11b.

## Ejecutar

```powershell
cargo test --release -p spikes --bin spike_device_battery --locked
cargo run --release -p spikes --bin spike_device_battery --locked -- --seconds 5 --cycles 3
cargo run --release -p spikes --bin spike_device_battery --locked -- --verify-timeout
```

El JSON sale por stdout; no se crean registros automáticos. Los nombres son los
que ofrece Windows; no se imprimen direcciones Bluetooth, identificadores de
instancia ni Container IDs. Si se guarda el informe para compartirlo, revisar
también los nombres elegidos por el usuario para sus dispositivos.

## Qué se prueba

- Enumeración SetupAPI limitada a los enumeradores Bluetooth, ratones y audio USB;
  sin barrer todas las colecciones HID, abrir dispositivos ni enviarles informes.
- Una sonda aislada de propiedades de batería/conexión de la caché de Windows.
  Esas propiedades Bluetooth no son un contrato público: una ausencia no prueba
  que el hardware sea incompatible ni autoriza a mostrar 0 %.
- Un `DeviceWatcher` WinRT limitado a dispositivos Bluetooth ya emparejados.
  Suscripciones a altas, cambios, bajas, final de enumeración y parada, en MTA.
  No se hace búsqueda BLE de vecinos, emparejamiento ni conexión GATT.
- Los callbacks solo encolan referencias en una cola acotada y no bloqueante.
  Una saturación invalida la captura; no se ocultan eventos perdidos. Las
  actualizaciones parciales se aplican mediante `DeviceInformation.Update`.
- Agrupación por Container ID, con identidad de instancia como respaldo. Nombres
  iguales no fusionan dispositivos. Una conexión desconocida sigue siendo
  desconocida; niveles contradictorios no se promedian.
- Supervisor con espera de kernel, salida máxima de 64 KiB y plazo global de
  observación más ocho segundos. Cancelar el hijo tiene una espera adicional
  limitada a un segundo. `--verify-timeout` prueba ese camino con un hijo propio
  detenido deliberadamente; no afecta a otros procesos.

La instantánea SetupAPI se obtiene al terminar la observación, no se repite por
fotograma ni periódicamente. Los ciclos son repeticiones explícitas del
diagnóstico para observar coste de inicialización y liberación.

## Interpretar el informe

| Campo | Qué demuestra y qué no |
|---|---|
| `devices` | Nodos presentes agrupados físicamente. Un ratón HID puede ser cableado o un receptor; su presencia no demuestra que un periférico inalámbrico esté conectado. |
| `percent: null` | No hay un nivel único válido en la caché consultada. No es 0 % ni una declaración de incompatibilidad. |
| `connected: null` | Windows no aportó un estado suficiente. Si se puede correlacionar, AEP tiene prioridad sobre la pista de un nodo de servicio. |
| `conflicting_levels` | Las interfaces del mismo contenedor ofrecen niveles distintos; no se elige uno arbitrariamente. |
| `notifications.endpoints` | Dispositivos Bluetooth emparejados, con categoría y conexión cuando Windows las entrega. Pueden estar desconectados. |
| `initial_enumeration_ms` | Tiempo desde `Start` hasta el final de la enumeración; no incluye crear el watcher y registrar callbacks. `null` significa captura incompleta, igual que `stopped_unexpectedly: true`; una lista vacía no demuestra ausencia de dispositivos. |
| `changed_properties` | Propiedades que Windows notificó durante esta observación. Aún hay que comprobar si los cambios de batería del hardware real llegan por esta vía o necesitan avisos PnP. |
| `idle_cpu_ms` / `idle_wall_ms` | CPU acumulada tras la enumeración inicial y duración de ese tramo. Solo interpretarlo como reposo si `changes_after_initial_enumeration` es cero y el escritorio estuvo estable. |
| `cached_query_ms` | Tiempo de la consulta final de caché, separado de la observación de eventos. |
| `after_cleanup` | Memoria privada, pico privado, CPU acumulada y handles del proceso auxiliar tras revocar el watcher. Las cachés globales de WinRT y los resultados retenidos pueden sobrevivir entre ciclos. |

Un contador de CPU que informe 0 ms durante una ventana corta no garantiza un
consumo absoluto de cero: hay límites de resolución y trabajo del sistema fuera
del proceso. Estas medidas describen el diagnóstico, no la futura integración.

No se obtiene el instante de medición del periférico, carga real, porcentaje por
auricular/estuche ni porcentaje de receptores propietarios. Se necesitan
dispositivos reales para verificar esos datos antes de implementar sus lectores.

## Validación

Las pruebas unitarias cubren 0 %, ausencia, niveles inválidos y contradictorios,
identidades distintas con el mismo nombre, orden de enumeración, conexiones
parciales, omisión de identificadores y saturación de eventos.

Ejecución local del 2026-11-19: Windows 11 Pro 26200, sin elevación. Nueve pruebas
pasaron; Clippy sin advertencias y compilación optimizada correctos.

| Caso | Evidencia | Alcance |
|---|---|---|
| Inventario de ratones HID | Dos nodos, porcentaje ausente y conexión inalámbrica desconocida | No confundir presencia de una interfaz con conexión del accesorio. |
| Inventario AEP emparejado | Dos endpoints de categorías equipo/teléfono, desconectados; enumeración inicial completa en 5–7 ms | Valida la enumeración y la suscripción, no batería de ratón/auriculares. |
| Tres ciclos de cinco segundos | Caché: 8/5/6 ms; CPU registrada en espera: 0/0/0 ms; privada tras limpieza: 2,977/3,359/3,051 MiB; handles: 213/214/214 | Sin actualizaciones posteriores ni paradas inesperadas. Son medidas del auxiliar, con servicios ya inicializados. |
| Bloqueo simulado | Supervisor devolvió `timeout_enforced: true` en 209 ms; no quedaron procesos | Valida el límite de espera y terminación del proceso, no un controlador físico defectuoso. |

Pico privado: 4,418 MiB. Baseline: 122 handles antes de crear el watcher. Las
cachés de WinRT explican parte del coste inicial, pero tres ciclos no descartan
fugas de largo plazo. Comparar avisos PnP más específicos antes de decidir el
seguimiento residente de 11b; estas cifras no prueban el presupuesto del dock.

Sin periféricos inalámbricos disponibles, quedan pendientes conexión/desconexión
real, cambios de batería, suspensión con accesorios y contraste con Windows o la
utilidad del fabricante. Windows 10 requiere una ejecución independiente.

## Continuación y reversión

La integración inicial 11b reutiliza las decisiones de aislamiento y eventos, sin
arrancar este diagnóstico con el dock. Su código, pruebas y límites están en
[`apps/dock/ACCESSORIES.md`](../apps/dock/ACCESSORIES.md). El porcentaje real y sus
avisos siguen pendientes de hardware; la propiedad de compatibilidad no garantiza
lectura universal. GATT y lectores específicos se evaluarán con dispositivos.

El límite de reversión es este prototipo: `spike_device_battery.rs`,
`src/device_battery/`, este documento y sus dependencias exclusivas en
`spikes/Cargo.toml`/`Cargo.lock`. Ese límite corresponde solo al prototipo 11a;
la integración automática 11b y los adaptadores iniciales 11c tienen sus propios
cambios funcionales.

Referencias: [propiedades de dispositivos](https://learn.microsoft.com/en-us/windows/apps/develop/devices-sensors/device-information-properties),
[DeviceWatcher](https://learn.microsoft.com/en-us/uwp/api/windows.devices.enumeration.devicewatcher),
[SetupDiGetClassDevsW](https://learn.microsoft.com/en-us/windows/win32/api/setupapi/nf-setupapi-setupdigetclassdevsw)
y [límites de cancelación de E/S](https://learn.microsoft.com/en-us/windows/win32/fileio/canceling-pending-i-o-operations).
