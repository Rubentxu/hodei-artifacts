//! Generates the protobuf/gRPC code for the Dgraph client API.
//!
//! The `.proto` vendored in `protos/` is the official Dgraph client API of
//! the version line this workspace targets (Dgraph v25). It is compiled at
//! build time with tonic/prost at pinned versions, so the wire contract of
//! the crate cannot drift unnoticed: bumping the proto or the runtime is a
//! deliberate change of this file plus the manifest, never a silent one.
//!
//! **Host requirement:** the build needs the Protocol Buffers compiler
//! (`protoc`) on `PATH`, because `tonic-build` shells out to it. Without it
//! the build stops with prost's own message, which names the missing tool
//! but not this crate; the requirement is stated here so a machine that
//! cannot build the workspace says why before it fails. `PROTOC` overrides
//! the lookup, and a `protoc` from 3.15 upwards satisfies the pinned
//! `prost`/`tonic` versions.

fn main() -> Result<(), std::io::Error> {
    let proto = "protos/api.proto";
    // Rerun when the contract itself changes.
    println!("cargo:rerun-if-changed={proto}");

    tonic_build::configure()
        // This crate is a client only: no server stubs, no extra surface.
        .build_server(false)
        .compile_protos(&[proto], &["protos"])?;
    Ok(())
}
