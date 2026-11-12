# Standards conformance fixtures (`hodei-evidence`)

Deterministic, **real** conformance fixtures for the `hodei-evidence` crate:
CycloneDX 1.7 SBOM ingestion, SPDX 3.0 ingestion, and in-toto / SLSA v1.2
attestation ingestion with **real DSSE signature verification** (ECDSA P-256,
SHA-256, DER-encoded).

Everything in this directory is a test fixture. It is committed on purpose so
that the Rust test-suite is reproducible byte-for-byte on any machine.

---

## ⚠️ WARNING — throwaway test key

`signing-key-private.pem` is a **throwaway test key**. It exists **only** to
sign fixture envelopes so that the verifier can be exercised against real
cryptography.

* The private key **must never** be used to sign anything real.
* The private key is **published in this repository**; it is therefore
  **publicly known and completely worthless** outside this test suite.
* There is **no value** in this key outside the test suite: no key management
  system, no CI identity, no signing service, no production service trusts it.
* Never reuse this key pair, this seed label, or this key id
  (`test-key-p256-01`) outside of `hodei-evidence` tests.
* The key id `test-key-p256-01` is a **test-only** identifier.

If a fixture ever needs to be regenerated, regenerate it with the deterministic
recipe below so that the key material stays stable across machines and CI runs.

---

## Files

| File | What it is |
| --- | --- |
| `README.md` | This document. |
| `signing-key-private.pem` | EC **P-256 / secp256r1** private key, **PKCS#8** PEM. Throwaway test key — see warning above. |
| `signing-key-public.pem` | Matching EC **P-256** public key, **SubjectPublicKeyInfo** PEM. |
| `signing-key-public-point.hex` | The **raw uncompressed public point** (65 bytes, hex, `04` prefix) for verifiers that consume the raw point form instead of PEM/SPKI. |
| `cyclonedx-1.7.json` | CycloneDX **1.7** SBOM (`bomFormat`/`specVersion` 1.7). The subject is the container image `web` (in `metadata.component`, with its PURL and its digest) and `components` holds `lib-a`, `lib-b`, `lib-c` and `lib-unknown`. The `dependencies` graph starts at the subject. |
| `spdx-3.0.json` | SPDX **3.0** document expressing **the same facts** as the CycloneDX SBOM: 5 packages (`web` with the same PURL and digest, `lib-a`, `lib-b`, `lib-c`, `lib-unknown`) and the same relationship graph. `DOCUMENT DESCRIBES` points at the subject only, and the dependencies are `DEPENDS_ON` relations. |
| `attestation.dsse.json` | Valid **DSSE** envelope (Sigstore style) over an in-toto Statement v1 / SLSA v1 provenance predicate, signed with the real key. |
| `attestation-invalid.dsse.json` | Same envelope, **corrupted signature** (last DER byte flipped). Must fail verification. |
| `attestation-unknown-key.dsse.json` | Same valid envelope but with `"keyid": "test-key-p256-unknown"`. The signature itself is still valid — only the key id is unknown. |

---

## The key pair

* Curve: **NIST P-256 / secp256r1** (`prime256v1`), ECDSA with SHA-256.
* Key id used in every DSSE signature: **`test-key-p256-01`**.
* Public point (65 raw bytes, `04 || X || Y`, hex):

```
046df5443f8cdb5e5e797f6b3347cb88b06c1874858b8933d64796565c911eb61bc26f50a5605b558a97c4607eafe65ff33217cad9ca4e1b0bcbceacd7309c6b35
```

### Deterministic derivation (so anybody can regenerate the same key)

The private scalar is **not** random. It is derived from a fixed label:

```python
from cryptography.hazmat.primitives.asymmetric import ec
import hashlib

ORDER = 0xFFFFFFFF00000000FFFFFFFFFFFFFFFFBCE6FAADA7179E84F3B9CAC2FC632551
scalar = int.from_bytes(
    hashlib.sha256(b"hodei-evidence-fixture-key-p256-v1").digest(), "big"
) % (ORDER - 1) + 1
key = ec.derive_private_key(scalar, ec.SECP256R1())
```

`signing-key-private.pem` is the PKCS#8 PEM of that key,
`signing-key-public.pem` is its SubjectPublicKeyInfo PEM.

---

## Digests used (recomputable)

Every digest in these fixtures is the **SHA-256 of a literal ASCII string**, so
a reviewer can recompute them with e.g. `printf 'web-1.0.0' | sha256sum`:

| Literal string hashed | SHA-256 | Used for |
| --- | --- | --- |
| `web-1.0.0` | `ac9da485d6eac8c161117ecd4155cf50bdb330c8241234d669aef1edba265a06` | CycloneDX subject component (`metadata.component`), CycloneDX `web` container component, SPDX `SPDXRef-Package-web` digest, in-toto subject digest |
| `lib-b-2.0.0` | `7228493599080d271c0d276600ae9a03ee0b154e853d9ff7689568ad0d73079d` | CycloneDX `lib-b` component hashes, SPDX `SPDXRef-Package-lib-b` digest |

The `sha1` values in the SLSA provenance source/resolved-dependency entries are
the fixed 40-zero placeholder `0000000000000000000000000000000000000000`
(invalid on purpose — a source-control commit that does not exist).

---

## Fixed instants

| Instant | Where |
| --- | --- |
| `2026-03-04T10:00:00Z` | SLSA `runDetails.metadata.startedOn` |
| `2026-03-04T10:12:00Z` | SLSA `runDetails.metadata.finishedOn` |
| `2026-03-04T10:15:00Z` | CycloneDX `metadata.timestamp`, SPDX `creationInfo.created` |

`invocationId` is the fixed literal `example-invocation-01`.

---

## The DSSE envelope

`attestation.dsse.json` is a Sigstore-style DSSE envelope:

```json
{
  "payloadType": "application/vnd.in-toto+json",
  "payload": "<base64 of the raw statement bytes>",
  "signatures": [ { "keyid": "test-key-p256-01", "sig": "<base64 of the DER ECDSA signature>" } ]
}
```

* `payload` — **standard base64 (with padding)** of the raw UTF-8 bytes of the
  in-toto Statement v1 JSON (2-space indented, trailing newline).
* `sig` — **standard base64 (with padding)** of the **DER**-encoded ECDSA
  signature (this is what `cryptography` produces by default).

### Exact PAE construction

The signature is computed over the **DSSE Pre-Authentication Encoding (PAE)**
of the payload — **not** over the raw payload bytes:

```
PAE = b"DSSEv1 "
    + str(len(payload_type)).encode()      # length of the payload type, ASCII
    + b" "
    + payload_type                          # UTF-8 bytes
    + b" "
    + str(len(payload_bytes)).encode()      # length of the RAW payload bytes, ASCII
    + b" "
    + payload_bytes                         # raw (decoded) payload bytes
```

with

* `payload_type = b"application/vnd.in-toto+json"` (length **28**)
* `payload_bytes` = the decoded payload (the exact bytes the `payload` field
  base64-encodes).

Reference implementation (this is exactly what produced the fixture):

```python
def pae(payload_type: str, payload: bytes) -> bytes:
    return (
        b"DSSEv1 "
        + str(len(payload_type)).encode()
        + b" "
        + payload_type.encode()
        + b" "
        + str(len(payload)).encode()
        + b" "
        + payload
    )

signature = private_key.sign(pae(payload_type, payload_bytes), ec.ECDSA(hashes.SHA256()))
```

Note: the PAE bytes are signed **directly** with `ECDSA(SHA256())` — no
prehashing, no `Prehashed` wrapper.

### Negative / edge fixtures

* `attestation-invalid.dsse.json` — identical to the valid envelope except
  that the **last byte of the DER signature** is flipped (`0x01` XOR). The DER
  structure stays parseable but the signature no longer verifies. Use it to
  assert that verification **fails** (not that parsing fails).
* `attestation-unknown-key.dsse.json` — identical to the valid envelope except
  `"keyid": "test-key-p256-unknown"`. The **signature itself still verifies**
  against `signing-key-public.pem`; only the key id is unknown. Use it to
  exercise the "unknown key id" rejection path in isolation from
  "bad signature".

---

## Fact model shared by `cyclonedx-1.7.json` and `spdx-3.0.json`

Subject: `web` version `1.0.0` (`pkg:generic/web@1.0.0`), sha256 of
`web-1.0.0`.

| Component | Type | PURL | sha256 |
| --- | --- | --- | --- |
| `web` (subject / metadata) | application | `pkg:generic/web@1.0.0` | `ac9da485…a06` |
| `web` (OCI image) | container | `pkg:oci/web@sha256:ac9da485…a06` | `ac9da485…a06` |
| `lib-a` | library | `pkg:maven/org.example/lib-a@1.2.3` | *(none)* |
| `lib-b` | library | `pkg:maven/org.example/lib-b@2.0.0` | `72284935…079d` |
| `lib-c` | library | `pkg:golang/github.com/example/lib-c@v1.4.2` | *(none)* |
| `lib-unknown` | library | *(no purl)* | *(none)* |

Dependency graph (identical in both documents):

```
web  -> lib-a -> lib-b -> lib-c
```

`lib-unknown` is the deliberate **"identity cannot be determined"** case: it
has **no purl and no digest**, so a resolver must not be able to derive a
stable identity for it.

### Notes on shape

* CycloneDX: `components` holds `lib-a`, `lib-b`, `lib-c`, the `web` OCI
  container and `lib-unknown`; the subject (`web`, application) lives in
  `metadata.component` with `bom-ref` `pkg:generic/web@1.0.0`. The
  `dependencies` array lists every `bom-ref` (including the subject) with its
  `dependsOn` list; components without outgoing edges carry an empty
  `dependsOn`.
* SPDX 3.0: `packages` mirrors the same set, keyed by `SPDXID`
  (`SPDXRef-Package-lib-a`, `-lib-b`, `-lib-c`, `-web`, `-lib-unknown`).
  PURLs are carried as `externalRefs` with `referenceCategory:
  "PACKAGE-MANAGER"` / `referenceType: "purl"`; digests as `externalRefs` with
  `referenceCategory: "SECURITY"` / `referenceType: "sha256"`. The `web`
  package carries **both** purls (the generic subject purl and the OCI image
  purl of the very same digest) so the SPDX document expresses the same facts
  as the CycloneDX document. `lib-unknown` carries **no** purl and **no**
  digest `externalRef`.
* SPDX relationships: `SPDXRef-DOCUMENT` `DESCRIBES` every package, plus
  `SPDXRef-Package-lib-a` `DEPENDS_ON` `SPDXRef-Package-lib-b`,
  `SPDXRef-Package-lib-b` `DEPENDS_ON` `SPDXRef-Package-lib-c` and
  `SPDXRef-Package-web` (the subject) `DEPENDS_ON` `SPDXRef-Package-lib-a`.
* SPDX `creationInfo.creators` is a single creator entry,
  `Tool: hodei-fixture-generator-1.0.0` (SPDX 3.0 creator syntax
  `Tool: <name>-<version>`).

All JSON files are pretty-printed with **2-space indentation** and end with a
**trailing newline**.


---

## The subject is a component

Both documents name the subject of the inventory differently: CycloneDX puts
it in `metadata.component` and SPDX puts it among the `packages`, with the
document pointing at it with a `DESCRIBES` relation. The native model of
Hodei has one rule for the two: **the subject is also a component of the
inventory**, because the dependency graph starts at it. A normalizer that
finds the subject only as the subject adds it to the components, so that the
edges can reach it.

The CycloneDX fixture names its subject with the OCI PURL
(`pkg:oci/web@sha256:ac9da48...`) and the SPDX fixture names the same
subject with the same PURL and the same digest, so the two documents
describe the same thing under the same identity.

The `lib-unknown` component is in both documents on purpose: it has a name
and a version and **no identity at all**, and it exercises the path where
Hodei must not invent one.
