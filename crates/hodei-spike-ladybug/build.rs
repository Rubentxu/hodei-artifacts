//! El enlazado del prototipo.
//!
//! La libreria precompilada de LadybugDB (lbug) trae el cliente HTTP con
//! TLS, y sus simbolos de OpenSSL hay que resolverlos al enlazar. La
//! libreria esta compilada con TLS y el enlazador no lo sabe, asi que
//! aqui se dice explicitamente.
//!
//! Solo se dice cuando la caracteristica esta activada: el producto no
//! depende de LadybugDB y no tiene por que pagar su enlazado.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_FEATURE_LADYBUG").is_ok() {
        // El argumento va al BINARIO: la libreria que se enlaza es la del
        // motor analitico, y la resolucion de sus simbolos es del binario
        // final.
        println!("cargo:rustc-link-arg-bins=-lssl");
        println!("cargo:rustc-link-arg-bins=-lcrypto");
    }
}
