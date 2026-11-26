//! The prototype's linking.
//!
//! The precompiled LadybugDB (lbug) library brings the HTTP client with
//! TLS, and its OpenSSL symbols must be resolved at link time. The
//! library is compiled with TLS and the linker does not know it, so
//! it is stated explicitly here.
//!
//! It is only stated when the feature is enabled: the product does not
//! depend on LadybugDB and has no reason to pay for its linking.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_FEATURE_LADYBUG").is_ok() {
        // The argument goes to the BINARY: the library being linked is the
        // analytical engine's, and the resolution of its symbols belongs to the
        // final binary.
        println!("cargo:rustc-link-arg-bins=-lssl");
        println!("cargo:rustc-link-arg-bins=-lcrypto");
    }
}
