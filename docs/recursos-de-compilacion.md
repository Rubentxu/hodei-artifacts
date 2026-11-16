# Recursos de compilación y tests

Este servidor es compartido: varios agentes compilan a la vez y hay otras
cargas (compilaciones de C++, el escritorio). Una compilación de este
workspace sin límite llegó a ~900 % de CPU (`rustc` y el enlazador `mold`),
porque cada binario de test se enlaza por separado y `mold` usa todos los
núcleos por defecto. Esta página fija los límites y la razón de cada uno.

## Cómo ejecutar

| Qué | Comando |
| --- | --- |
| Cualquier `cargo` largo | `cargo-lim test --workspace --no-fail-fast` |
| El pipeline local | `bounded pipelinek run` |
| Cualquier otro comando pesado | `bounded <comando>` |

`bounded` (`~/.local/bin/bounded`) pone todo el árbol de procesos bajo un
cgroup de usuario con tope y prioridad baja; `cargo-lim` es `bounded cargo`.
Los límites están en esos ficheros y en `~/.cargo/config.toml`, no en este
repositorio: son una política de la máquina, no del código. `pipeline.kts` no
se envuelve por dentro porque `tests/ci_suites.rs` lee sus comandos `cargo
test` literales; el límite se aplica por fuera y el pipeline lo hereda.

## Valores

| Límite | Valor | Dónde | Por qué |
| --- | --- | --- | --- |
| Trabajos de `rustc` | `build.jobs = 4` | `~/.cargo/config.toml` | Cuatro agentes × cuatro trabajos = 16 `rustc` como máximo sobre 64 núcleos. No cambia la huella de ningún artefacto: tocarlo no recompila. |
| Hilos del enlazador | `--thread-count=4` | `~/.cargo/bin/cc-bounded` | `mold` usa todos los núcleos por enlace; con un enlace por binario de test saturaba la máquina. Solo se pasa a `mold`: `rust-lld` (doctests) no entiende la opción. |
| Hilos de test | `RUST_TEST_THREADS = 8` | `~/.cargo/config.toml` (`[env]`, `force = false`) | El valor por defecto es un hilo por núcleo (64) por binario. Se puede subir puntualmente con la variable de entorno. |
| CPU, tope duro | `CPUQuota=600%` (6 núcleos) | `bounded` (`BOUNDED_CPU`) | Cubre los 4 `rustc` más el enlace sin apretar. Medido el 2026-11-16 en un build que recompila `hodei-catalog` y reenlaza todos los tests: pico ~600 % (antes ~900 % con tope de 12). Cuatro agentes a la vez suman como mucho 24 de 64 núcleos. |
| CPU, prioridad | `CPUWeight=50` y `nice -n 10` | `bounded`, `cc-bounded` | Solo cuenta cuando hay competencia: ceden ante cualquier otra carga del servidor, y no frenan si está libre. |
| Disco, prioridad | `IOWeight=50` y `ionice -c2 -n7` | `bounded`, `cc-bounded` | Los enlaces y el `target-dir` compartido escriben mucho; se cede el disco a lo demás. |
| Memoria | `MemoryHigh=28G`, `MemoryMax=32G`, sin swap | `bounded` (`BOUNDED_MEM`) | Pico medido de un `rustc` de este workspace: ~300 MB. El tope no limita el build normal: evita que un fallo o un test desbocado provoque un OOM en la máquina; el kernel mata este árbol, no el resto. |
| Procesos | `TasksMax=4096` | `bounded` | Un test que lance procesos sin control no agota los PID del usuario. |
| Caché `sccache` | `SCCACHE_CACHE_SIZE = 5G` | `~/.cargo/config.toml` | La tasa de acierto en Rust es del 0,67 %: la caché ocupa disco y casi no devuelve. |

## Por qué un cgroup y no solo `nice`

`nice` e `ionice` solo reordenan cuando hay competencia: un build con `nice`
sigue usando 900 % si la máquina está libre en ese instante, y compite con
otras compilaciones en cuanto no lo está. El cgroup (`CPUQuota`) es un techo
que se cumple siempre. Se usan los dos: el techo acota el daño máximo y la
prioridad hace que el resto del servidor tenga preferencia.

## Qué no se toca

- `codegen-units = 256` y `debug = "line-tables-only"` del perfil de
  desarrollo: entran en la huella de cada artefacto; cambiarlos recompila
  todo en todos los proyectos. Se cambian solo en una ventana de mantenimiento.
- `build.jobs` no se fija en un `.cargo/config.toml` del repositorio: un
  límite pensado para este servidor ralentizaría a quien compile en una
  máquina libre. La política vive en la máquina.

## Cómo comprobarlo

```sh
bounded sh -c 'cat /sys/fs/cgroup$(cut -d: -f3 /proc/self/cgroup)/cpu.max; nice; ionice'
# 600000 100000 / 10 / best-effort: prio 7
```

Para medir el pico de un build, leer `usage_usec` de `cpu.stat` del scope
`bounded-<pid>` cada dos segundos.
