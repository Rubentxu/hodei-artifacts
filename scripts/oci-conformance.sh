#!/usr/bin/env bash
# La suite oficial de conformidad del `distribution-spec` (HSP-103 / HAR-09).
#
# La suite **no** esta en el repositorio, y es a proposito: es codigo de
# OpenContainers, con su licencia y su ritmo, y copiarla al repositorio
# seria mantener un fork que nadie ha pedido y que se quedaria viejo sin
# que nadie se entere. Lo que si esta aqui es el **commit** exacto del que
# se usa y el **digest** de sus fuentes: lo que se ejecuta tiene que ser
# siempre lo mismo, y un commit sin digest no dice nada.
#
# El binario se deja en `target/`, que es donde vive lo que se genera, y lo
# genera la pipeline antes de la suite E2E.
#
# Lo que queda fijado es **el codigo que se ejecuta**--el commit y el digest
# de sus fuentes--, no el compilador: el `go` es el de la maquina, y la
# version se dice en el preflight para que se pueda ver.
#
# Codigos de salida:
#   0  la suite esta descargada, verificada y compilada
#   1  no se puede descargar el codigo
#   2  las fuentes no son las fijadas: el digest no coincide
#   3  no se puede compilar la suite
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
DEST="$ROOT/target/oci-conformance"
BIN="$DEST/oci-conformance"

# El commit de la suite que se ejecuta.
COMMIT="97274622c11112caa21efb8c52acca3c6b8fa7f1"

# El digest de los ficheros de la suite: el mismo commit con otro
# contenido no es el mismo codigo, y un digest que no se comprueba no dice
# nada.
#
# El digest se calcula con el `sort` en locale `C`: el orden del `sort`
# depende del locale de la maquina, y un digest que cambia con el locale
# es un digest que falla en otra maquina. Por eso el valor de aqui es el
# de `LC_ALL=C`, y no el que sale con el locale de quien lo calculo.
DIGEST="898a7fc644c5eae0accd6fc96fc019196ddd4c56eac72d1e14b0450c70a52ff5"

# El digest de los ficheros de la suite. El `sort` va con `LC_ALL=C`:
# el orden del `sort` depende del locale, y un digest que cambia con el
# locale es un digest que falla en otra maquina.
the_sources_digest() {
    (cd "$DEST" && find conformance -type f | LC_ALL=C sort | xargs sha256sum | sha256sum | cut -d' ' -f1)
}

if [ ! -d "$DEST/conformance" ]; then
    rm -rf "$DEST"
    mkdir -p "$DEST"

    if ! curl -fsSL --proto '=https' --tlsv1.2 --max-time 300 --retry 3 \
        "https://codeload.github.com/opencontainers/distribution-spec/tar.gz/$COMMIT" \
        | tar xz -C "$DEST" --strip-components=1; then
        echo "the conformance suite could not be downloaded" >&2
        exit 1
    fi
fi

ACTUAL="$(the_sources_digest)"
if [ "$ACTUAL" != "$DIGEST" ]; then
    # Lo que hay no es lo fijado: se vuelve a descargar. Un cache viejo
    # no es un motivo para que la etapa falle, es un motivo para dejar de
    # usarlo.
    echo "the conformance sources are not the pinned ones: $ACTUAL != $DIGEST; downloading again" >&2
    rm -rf "$DEST"
    mkdir -p "$DEST"

    if ! curl -fsSL --proto '=https' --tlsv1.2 --max-time 300 --retry 3 \
        "https://codeload.github.com/opencontainers/distribution-spec/tar.gz/$COMMIT" \
        | tar xz -C "$DEST" --strip-components=1; then
        echo "the conformance suite could not be downloaded" >&2
        exit 1
    fi

    ACTUAL="$(the_sources_digest)"
    if [ "$ACTUAL" != "$DIGEST" ]; then
        echo "the conformance sources are not the pinned ones: $ACTUAL != $DIGEST" >&2
        exit 2
    fi
fi

# El binario se compila **siempre**. Comprobar el digest de las fuentes y
# ejecutarse un binario viejo no seria comprobar nada: lo que se
# verifica tiene que ser lo que se ejecuta.
if ! (cd "$DEST/conformance" && go build -o "$BIN" .); then
    echo "the conformance suite could not be built" >&2
    exit 3
fi
