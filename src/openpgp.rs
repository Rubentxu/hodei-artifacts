//! The minimum OpenPGP needed to sign system metadata:
//! an Ed25519 key, its publication as a keyring that `gpgv` accepts, and
//! armored detached signatures over APT's `Release` or
//! yum's `repomd.xml`.
//!
//! This is not an in-house implementation of the format: packet layout
//! and signing are done by the `pgp` crate, and `gpgv` is the oracle in
//! the tests. That is deliberate: a home-grown OpenPGP layout that `gpg`
//! does not validate is exactly the failure the CA wants to avoid, and apt does not
//! negotiate: a malformed signature means "the repository does not exist".
//!
//! The key is generated once per process and travels in memory as
//! `MetadataSigner`; the private material can be serialized in armored form
//! and loaded back, which is how an operator persists the identity
//! across restarts.

use pgp::composed::{
    CleartextSignedMessage, Deserializable, DetachedSignature, KeyType, SecretKeyParamsBuilder,
    SignedPublicKey, SignedSecretKey,
};
use pgp::crypto::hash::HashAlgorithm;
use pgp::ser::Serialize;
use pgp::types::{KeyDetails, Password};
use rand::thread_rng;

/// The signer's failures: building or loading the key, and signing. Signing
/// cannot fail with an already loaded key, but the format says so
/// with its type and here it is translated into something of our own.
#[derive(Debug, thiserror::Error)]
pub enum OpenPgpError {
    #[error("the key could not be built: {0}")]
    Key(String),
    #[error("the armored key is not a readable OpenPGP secret key")]
    NotASecretKey,
    #[error("the signature could not be computed: {0}")]
    Signing(String),
    #[error("the armored text is not a readable OpenPGP signature")]
    NotASignature,
    #[error("the armored text is not a readable OpenPGP public key")]
    NotAPublicKey,
    #[error("the bytes are not a readable OpenPGP cleartext-signed message")]
    NotACleartextMessage,
    #[error("the signature does not hold over these bytes with this key: {0}")]
    Verification(String),
}

/// The key with which the hub signs the metadata of the system repositories,
/// plus its public certificate. The `uid` is what the distro
/// tools show as the signer.
#[derive(Debug, Clone)]
pub struct MetadataSigner {
    secret: SignedSecretKey,
    public: SignedPublicKey,
}

/// Operations required to publish signed repository metadata.
pub trait MetadataSigning: Send + Sync {
    /// Return the public key in armored form.
    fn key_asc(&self) -> Result<String, OpenPgpError>;
    /// Return the public key fingerprint in uppercase hexadecimal.
    fn fingerprint_hex(&self) -> String;
    /// Sign bytes as an armored detached signature.
    fn sign_detached(&self, data: &[u8]) -> Result<Vec<u8>, OpenPgpError>;
    /// Sign UTF-8 text as an armored cleartext message.
    fn sign_cleartext(&self, text: &str) -> Result<Vec<u8>, OpenPgpError>;
}

impl MetadataSigning for MetadataSigner {
    fn key_asc(&self) -> Result<String, OpenPgpError> {
        MetadataSigner::key_asc(self)
    }
    fn fingerprint_hex(&self) -> String {
        MetadataSigner::fingerprint_hex(self)
    }
    fn sign_detached(&self, data: &[u8]) -> Result<Vec<u8>, OpenPgpError> {
        MetadataSigner::sign_detached(self, data)
    }
    fn sign_cleartext(&self, text: &str) -> Result<Vec<u8>, OpenPgpError> {
        MetadataSigner::sign_cleartext(self, text)
    }
}

impl MetadataSigner {
    /// Generates a new Ed25519 key with the given identity. The primary
    /// signature is the key's own (no subkeys): apt and rpm only
    /// need to find the key by its fingerprint in the keyring.
    pub fn generate(uid: &str) -> Result<Self, OpenPgpError> {
        let mut params = SecretKeyParamsBuilder::default();
        params
            .key_type(KeyType::Ed25519Legacy)
            .can_certify(true)
            .can_sign(true)
            .primary_user_id(uid.into());
        let secret = params
            .build()
            .map_err(|error| OpenPgpError::Key(error.to_string()))?
            .generate(thread_rng())
            .map_err(|error| OpenPgpError::Key(error.to_string()))?;
        let secret = secret
            .sign(&mut thread_rng(), &Password::from(""))
            .map_err(|error| OpenPgpError::Key(error.to_string()))?;
        let public = SignedPublicKey::from(secret.clone());
        Ok(Self { secret, public })
    }

    /// Recovers the signer from the armored text of a private key
    /// exported with `to_armor`. This is the persistence path:
    /// the operator stores the file and the hub signs again with the
    /// same identity on every start.
    pub fn from_armor(armored: &str) -> Result<Self, OpenPgpError> {
        let (secret, _) = SignedSecretKey::from_armor_single(std::io::Cursor::new(armored))
            .map_err(|_| OpenPgpError::NotASecretKey)?;
        let public = SignedPublicKey::from(secret.clone());
        Ok(Self { secret, public })
    }

    /// The private key in armored text, for persisting it.
    pub fn to_armor(&self) -> Result<String, OpenPgpError> {
        self.secret
            .to_armored_string(pgp::composed::ArmorOptions::default())
            .map_err(|error| OpenPgpError::Key(error.to_string()))
    }

    /// The public keyring in binary: it is what `gpgv
    /// --keyring` reads, and what apt expects in `TrustedPart` when it is
    /// exported in the clear; for the `.asc` file installed in
    /// `signed-by` use `key_asc`.
    pub fn keyring(&self) -> Result<Vec<u8>, OpenPgpError> {
        self.public
            .to_bytes()
            .map_err(|error| OpenPgpError::Key(error.to_string()))
    }

    /// The public key in armored text: the `key.asc` that the
    /// distro administrator references with `signed-by`.
    pub fn key_asc(&self) -> Result<String, OpenPgpError> {
        self.public
            .to_armored_string(pgp::composed::ArmorOptions::default())
            .map_err(|error| OpenPgpError::Key(error.to_string()))
    }

    /// The fingerprint in uppercase hexadecimal, as printed by `gpg
    /// --list-keys` and `dnf`; it serves for audit logs and so
    /// that the reader knows which key to expect in the keyring.
    pub fn fingerprint_hex(&self) -> String {
        self.public.fingerprint().to_string().to_uppercase()
    }

    /// The detached signature in armored text (type `Binary`, SHA-256)
    /// over the exact bytes of the metadata: `Release.gpg` and
    /// `repomd.xml.asc` are exactly this over the body that is
    /// served in the same request. The armor is not decoration: apt
    /// concatenates signature and message for gpgv, and only the armored format
    /// is recognized there as a detached signature.
    /// Signs UTF-8 metadata as an OpenPGP cleartext-signed message.
    pub fn sign_cleartext(&self, text: &str) -> Result<Vec<u8>, OpenPgpError> {
        pgp::composed::CleartextSignedMessage::sign(
            thread_rng(),
            text,
            &self.secret.primary_key,
            &Password::from(""),
        )
        .and_then(|message| message.to_armored_bytes(pgp::composed::ArmorOptions::default()))
        .map_err(|error| OpenPgpError::Signing(error.to_string()))
    }

    pub fn sign_detached(&self, data: &[u8]) -> Result<Vec<u8>, OpenPgpError> {
        let signature = DetachedSignature::sign_binary_data(
            thread_rng(),
            &self.secret.primary_key,
            &Password::from(""),
            HashAlgorithm::Sha256,
            std::io::Cursor::new(data.to_vec()),
        )
        .map_err(|error| OpenPgpError::Signing(error.to_string()))?;
        signature
            .to_armored_string(pgp::composed::ArmorOptions::default())
            .map(String::into_bytes)
            .map_err(|error| OpenPgpError::Signing(error.to_string()))
    }
    /// Checks an armored detached signature over the exact bytes
    /// given, against an armored public key. It is the inverse of
    /// `sign_detached`, and whoever receives a package over the air needs it:
    /// there is no `gpgv` there, but the same types are.
    pub fn verify_detached(
        data: &[u8],
        signature_armor: &[u8],
        public_key_asc: &str,
    ) -> Result<(), OpenPgpError> {
        let (signature, _) =
            DetachedSignature::from_armor_single(std::io::Cursor::new(signature_armor.to_vec()))
                .map_err(|_| OpenPgpError::NotASignature)?;
        let (key, _) = SignedPublicKey::from_armor_single(std::io::Cursor::new(public_key_asc))
            .map_err(|_| OpenPgpError::NotAPublicKey)?;
        key.verify()
            .map_err(|error| OpenPgpError::Verification(error.to_string()))?;
        if signature.verify(&key.primary_key, data).is_ok()
            || key
                .public_subkeys
                .iter()
                .any(|subkey| signature.verify(&subkey.key, data).is_ok())
        {
            Ok(())
        } else {
            Err(OpenPgpError::Verification(
                "no key in the trusted certificate verifies the detached signature".to_owned(),
            ))
        }
    }

    /// Verifies an OpenPGP cleartext-signed message against an operator-pinned certificate.
    pub fn verify_cleartext(data: &[u8], public_key_asc: &str) -> Result<(), OpenPgpError> {
        Self::verify_cleartext_text(data, public_key_asc).map(|_| ())
    }

    /// Like [`Self::verify_cleartext`], and returns the text the signature
    /// covers (line endings normalized to `\n`): what a caller must parse so
    /// that it reads exactly what was verified and nothing else.
    pub fn verify_cleartext_text(
        data: &[u8],
        public_key_asc: &str,
    ) -> Result<String, OpenPgpError> {
        let message = CleartextSignedMessage::from_armor(std::io::Cursor::new(data))
            .map(|(message, _)| message)
            .map_err(|_| OpenPgpError::NotACleartextMessage)?;
        let (key, _) = SignedPublicKey::from_armor_single(std::io::Cursor::new(public_key_asc))
            .map_err(|_| OpenPgpError::NotAPublicKey)?;
        key.verify()
            .map_err(|error| OpenPgpError::Verification(error.to_string()))?;
        if message.verify(&key.primary_key).is_ok()
            || key
                .public_subkeys
                .iter()
                .any(|subkey| message.verify(&subkey.key).is_ok())
        {
            Ok(message.signed_text().replace("\r\n", "\n"))
        } else {
            Err(OpenPgpError::Verification(
                "no key in the trusted certificate verifies the cleartext signature".to_owned(),
            ))
        }
    }
}

/// The signer the process starts with. If the operator leaves the
/// key in `HODEI_METADATA_SIGNER_ARMOR` (the armored text
/// returned by `to_armor`), that is the hub's identity; if not, an
/// ephemeral one is generated: each start signs with a new key, and the client
/// has to read the one served at `key.asc`, which is how it works
/// in a test and how it works in a single-node hub.
pub fn bootstrap_signer() -> MetadataSigner {
    if let Ok(armor) = std::env::var("HODEI_METADATA_SIGNER_ARMOR") {
        return MetadataSigner::from_armor(&armor)
            .expect("HODEI_METADATA_SIGNER_ARMOR must hold an armored OpenPGP secret key");
    }
    MetadataSigner::generate("Hodei Security Platform <metadata@hodei.local>")
        .expect("an Ed25519 signing key can always be generated")
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELEASE: &[u8] = b"Origin: Hodei\nSuite: stable\nCodename: hodei\nDate: Tue, 17 Nov 2026 18:00:00 UTC\nArchitectures: amd64 arm64\nComponents: main\n";

    fn parse_detached(signature: &[u8]) -> DetachedSignature {
        let (detached, _) = DetachedSignature::from_armor_single(std::io::Cursor::new(signature))
            .expect("a standalone armored signature");
        detached
    }

    #[test]
    fn a_generated_key_signs_and_the_crate_verifies() {
        let signer = MetadataSigner::generate("Hodei <hodei@example.local>").expect("a key");
        let signature = signer.sign_detached(RELEASE).expect("a signature");
        let detached = parse_detached(&signature);
        detached
            .verify(&signer.public, RELEASE)
            .expect("the signature opens with the public key");
    }

    #[test]
    fn the_signer_survives_the_armor_round_trip() {
        let signer = MetadataSigner::generate("Hodei <hodei@example.local>").expect("a key");
        let armor = signer.to_armor().expect("armored secret");
        let loaded = MetadataSigner::from_armor(&armor).expect("the same key loads");
        assert_eq!(signer.fingerprint_hex(), loaded.fingerprint_hex());
        let signature = loaded.sign_detached(RELEASE).expect("it still signs");
        let detached = parse_detached(&signature);
        detached
            .verify(&signer.public, RELEASE)
            .expect("the original key opens what the reloaded one signed");
    }

    #[test]
    fn pinned_certificate_verifies_cleartext_and_rejects_tampering() {
        let signer = MetadataSigner::generate("Hodei <hodei@example.local>").expect("a key");
        let public_key = signer.key_asc().expect("public key armor");
        let signed = signer
            .sign_cleartext(std::str::from_utf8(RELEASE).expect("UTF-8 release"))
            .expect("cleartext signature");

        MetadataSigner::verify_cleartext(&signed, &public_key)
            .expect("the pinned certificate opens the signed message");

        let tampered = String::from_utf8(signed)
            .expect("signed message is UTF-8")
            .replace("Suite: stable", "Suite: unstable");
        assert!(
            MetadataSigner::verify_cleartext(tampered.as_bytes(), &public_key).is_err(),
            "a changed Release must not verify"
        );
    }

    #[test]
    fn a_signature_over_other_bytes_does_not_open() {
        let signer = MetadataSigner::generate("Hodei <hodei@example.local>").expect("a key");
        let signature = signer.sign_detached(RELEASE).expect("a signature");
        let detached = parse_detached(&signature);
        assert!(
            detached
                .verify(&signer.public, b"Origin: Nobody\n")
                .is_err()
        );
    }

    /// The real oracle: `gpgv`, the same verifier that apt
    /// invokes behind the scenes, validates the signature against the keyring we
    /// serve. Without `gpgv` on the machine, the test is skipped.
    #[test]
    fn gpgv_opens_what_we_sign() {
        if which("gpgv").is_none() {
            eprintln!("skipping: gpgv is not installed");
            return;
        }
        let signer = MetadataSigner::generate("Hodei <hodei@example.local>").expect("a key");
        let dir = std::env::temp_dir().join(format!("hodei-gpgv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a scratch dir");
        let keyring = dir.join("key.bin");
        let message = dir.join("Release");
        let signature = dir.join("Release.gpg");
        std::fs::write(&keyring, signer.keyring().expect("a keyring")).expect("write keyring");
        std::fs::write(&message, RELEASE).expect("write release");
        std::fs::write(
            &signature,
            signer.sign_detached(RELEASE).expect("a signature"),
        )
        .expect("write signature");
        let output = std::process::Command::new("gpgv")
            .arg("--keyring")
            .arg(&keyring)
            .arg(&signature)
            .arg(&message)
            // The success message is localized; the locale is pinned so
            // we can check that gpgv literally said good signature.
            .env("LC_ALL", "C")
            .output()
            .expect("gpgv runs");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success() && stderr.contains("Good signature"),
            "gpgv said: {stderr}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The wire format for `Release.gpg` is the detached signature
    /// in armored text: `apt-key verify` (the wrapper apt
    /// invokes) only accepts it that way, and the armor is what makes it
    /// inspectable by eye in the repository.
    #[test]
    fn the_detached_signature_is_armored_the_way_apt_expects() {
        let signer = MetadataSigner::generate("Hodei <hodei@example.local>").expect("a key");
        let signature = signer.sign_detached(RELEASE).expect("a signature");
        let text = String::from_utf8(signature.clone()).expect("armor is ascii");
        assert!(
            text.starts_with("-----BEGIN PGP SIGNATURE-----"),
            "apt hands the file to apt-key verify, which requires armor"
        );
        if which("gpgv").is_none() {
            eprintln!("skipping the gpgv step: gpgv is not installed");
            return;
        }
        let dir = std::env::temp_dir().join(format!("hodei-armor-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a scratch dir");
        let keyring = dir.join("key.bin");
        let message = dir.join("Release");
        let sigfile = dir.join("Release.gpg");
        std::fs::write(&keyring, signer.keyring().expect("a keyring")).expect("write keyring");
        std::fs::write(&message, RELEASE).expect("write release");
        std::fs::write(&sigfile, &signature).expect("write signature");
        let output = std::process::Command::new("gpgv")
            .arg("--keyring")
            .arg(&keyring)
            .arg(&sigfile)
            .arg(&message)
            .env("LC_ALL", "C")
            .output()
            .expect("gpgv runs");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success() && stderr.contains("Good signature"),
            "gpgv said: {stderr}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn which(program: &str) -> Option<std::path::PathBuf> {
        std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths).find_map(|dir| {
                let candidate = dir.join(program);
                candidate.is_file().then_some(candidate)
            })
        })
    }
}
