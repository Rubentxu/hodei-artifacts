#!/usr/bin/env bash
# Las herramientas que necesita la suite E2E de OCI (HSP-103 / HAR-09).
#
# La CI de este repositorio es local, y una herramienta que falta se ve
# como un fallo del registro: el `assert` que falla dice "the image could
# not be built", y el motivo real es que no hay `podman`. Este script dice
# que falta **antes** de la suite, y con su nombre.
#
# La comprobacion del emulador de `aarch64` tambien esta: el caso de la
# imagen multi-plataforma construye una imagen de otra arquitectura, y sin
# el emulador el fallo aparece como un fallo de la construccion de la
# imagen, que no es lo que paso.
#
# Codigos de salida:
#   0  estan todas las herramientas
#   1  falta alguna
set -euo pipefail

missing=0

report() {
    printf '%s: %s\n' "$1" "$2"
}

# La version de cada herramienta, que cada una dice a su manera.
version_of() {
    case "$1" in
        podman) podman --version 2>&1 | head -1 ;;
        docker) docker --version 2>&1 | head -1 ;;
        oras) oras version 2>&1 | head -1 ;;
        go) go version 2>&1 | head -1 ;;
    esac
}

for tool in podman docker oras go; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "the tool '$tool' is not installed: the OCI E2E suite needs it" >&2
        missing=1
        continue
    fi
    report "$tool" "$(version_of "$tool")"
done

# El emulador de otra arquitectura: solo hace falta si la maquina no es
# ya de esa arquitectura.
if [ "$(uname -m)" != "aarch64" ]; then
    emulator=""
    for candidate in /usr/bin/qemu-aarch64-static /usr/libexec/qemu-binfmt/aarch64-binfmt-P; do
        if [ -e "$candidate" ]; then
            emulator="$candidate"
            break
        fi
    done
    if [ -z "$emulator" ]; then
        echo "the aarch64 emulator is not installed: the multi-architecture case needs it" >&2
        missing=1
    else
        report "qemu-aarch64" "$emulator"
    fi
fi

exit "$missing"
