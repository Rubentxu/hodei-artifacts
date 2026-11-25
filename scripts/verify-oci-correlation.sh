#!/usr/bin/env bash
# Verifies, from the directory that holds a served release bundle and nothing
# else, that the assets are what the manifests say they are (HSP-287):
#
#   1. `sha256sum -c SHA256SUMS` over every listed asset.
#   2. For every `*.manifest.json`: the OCI archive carries exactly one image
#      manifest whose digest is `oci.archive_manifest_digest`, whose config
#      architecture is the one the manifest declares, and whose platform label,
#      tarball triple, `.digest` file and provenance all name that same
#      architecture and image digest.
#   3. When the bundle has several architectures, an OCI image index (manifest
#      list) must exist and pin exactly the per-architecture image manifests,
#      with the right `platform.architecture` for each.
#
# It reads only the files of the directory (plus sha256sum, tar and python3):
# run it from an empty directory outside the source tree with the assets copied
# in, so no workspace state can mask a missing or swapped asset. Signatures are
# the job of scripts/verify-release.sh.
#
# Usage: scripts/verify-oci-correlation.sh <bundle-directory>

set -euo pipefail

# Stable tool messages (sha256sum reports in the locale of the host otherwise).
export LC_ALL=C

DIR="${1:-}"
if [ -z "$DIR" ] || [ ! -d "$DIR" ]; then
  echo "usage: scripts/verify-oci-correlation.sh <bundle-directory>" >&2
  exit 1
fi
for tool in sha256sum python3; do
  command -v "$tool" >/dev/null 2>&1 || { echo "error: $tool is required" >&2; exit 1; }
done
[ -f "$DIR/SHA256SUMS" ] || { echo "error: SHA256SUMS missing in $DIR" >&2; exit 1; }

( cd "$DIR" && sha256sum -c SHA256SUMS )

python3 -I - "$DIR" <<'PY'
import glob
import json
import os
import sys
import tarfile

directory = sys.argv[1]
GOARCH = {"x86_64": "amd64", "aarch64": "arm64"}


def die(msg):
    raise SystemExit(f"error: {msg}")


def load(name):
    with open(os.path.join(directory, name), encoding="utf-8") as f:
        return json.load(f)


paths = sorted(glob.glob(os.path.join(directory, "*.manifest.json")))
if not paths:
    die("no *.manifest.json in the bundle")

per_arch = {}
for path in paths:
    name = os.path.basename(path)
    m = load(name)
    arch = m["arch"]
    if arch not in GOARCH:
        die(f"{name}: unsupported architecture {arch!r}")
    if arch in per_arch:
        die(f"{name}: two manifests for architecture {arch}")
    oci = m["oci"]

    triple = m["tarball"]["filename"].removeprefix(f"{m['name']}-{m['version']}-").removesuffix(".tar.gz")
    if not triple.startswith(f"{arch}-"):
        die(f"{name}: tarball {m['tarball']['filename']} is not for architecture {arch}")
    if f"-{arch}-" not in oci["archive"]:
        die(f"{name}: OCI archive {oci['archive']} is not named for architecture {arch}")
    if oci.get("platform") != f"linux/{GOARCH[arch]}":
        die(f"{name}: oci.platform {oci.get('platform')!r} does not match {arch}")

    with tarfile.open(os.path.join(directory, oci["archive"])) as tf:
        index = json.load(tf.extractfile("index.json"))
        if len(index["manifests"]) != 1:
            die(f"{name}: the OCI archive must hold exactly one image manifest")
        digest = index["manifests"][0]["digest"]
        image = json.load(tf.extractfile("blobs/sha256/" + digest.split(":", 1)[1]))
        config = json.load(tf.extractfile("blobs/sha256/" + image["config"]["digest"].split(":", 1)[1]))
    if digest != oci.get("archive_manifest_digest"):
        die(f"{name}: archive image manifest {digest} != oci.archive_manifest_digest {oci.get('archive_manifest_digest')}")
    if config["architecture"] != GOARCH[arch] or config["os"] != "linux":
        die(f"{name}: image config is {config['os']}/{config['architecture']}, manifest says linux/{GOARCH[arch]}")
    if oci.get("archive_config_arch") != config["architecture"]:
        die(f"{name}: oci.archive_config_arch does not match the image config")

    digest_file = oci["archive"].removesuffix(".tar") + ".digest"
    with open(os.path.join(directory, digest_file), encoding="utf-8") as f:
        if f.read().strip() != oci["digest"]:
            die(f"{name}: {digest_file} differs from oci.digest")

    prov = load(m["provenance"]["filename"])
    params = prov["predicate"]["buildDefinition"]["externalParameters"]
    if params["arch"] != arch or params["oci_image_digest"] != oci["digest"]:
        die(f"{name}: provenance names arch {params['arch']} / digest {params['oci_image_digest']}")
    per_arch[arch] = (m, digest)
    print(f"ok - {arch}: archive manifest {digest} is linux/{config['architecture']}, "
          f"tarball {triple}, provenance and .digest agree")

indexes = sorted(glob.glob(os.path.join(directory, "*-oci-index.json")))
if len(per_arch) > 1 and not indexes:
    die("several architectures but no OCI image index (manifest list)")
if indexes:
    if len(indexes) != 1:
        die("more than one OCI image index")
    idx = load(os.path.basename(indexes[0]))
    if idx.get("mediaType") != "application/vnd.oci.image.index.v1+json":
        die("the image index has the wrong mediaType")
    seen = {}
    for entry in idx["manifests"]:
        platform = entry["platform"]
        if platform["os"] != "linux" or platform["architecture"] in seen:
            die(f"unexpected or duplicate platform in the image index: {platform}")
        seen[platform["architecture"]] = entry["digest"]
    expected = {GOARCH[a]: d for a, (_, d) in per_arch.items()}
    if seen != expected:
        die(f"the image index {seen} does not pin the per-architecture image manifests {expected}")
    print(f"ok - image index pins {sorted(seen)} with the digests of the per-architecture archives")
PY
echo "OK: $DIR is a correlated release bundle"
