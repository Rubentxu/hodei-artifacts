//! HSP-213: the production binary rejects the embedded profile.
//!
//! The half of the cut that is observed with the **real process**, not with a
//! unit that simulates it: a deployment without `[graph]` (the old
//! `HODEI_ROCKSDB__PATH` profile) dies with the migration message: it names the
//! retirement, the external authority and the retirement of the operations
//! tools (HSP-314 F4). And it creates nothing locally: the refusal comes before
//! touching the content store or any path of the old profile.
//!
//! Since HSP-314 F4 the graph profile is the only one: there is no binary
//! variant that can start the embedded profile, so this rejection is
//! the permanent contract of a deployment without `[graph]`.

mod hsp213_profile_env;

use hsp213_profile_env::{BIN, free_port, retired_profile_env};
use std::process::{Command, Stdio};

/// AC2: the production binary rejects the embedded profile with the
/// migration message and does not touch the local store.
#[test]
fn the_production_binary_refuses_the_embedded_profile_with_the_migration_message() {
    let tmp = tempfile::tempdir().expect("a temp dir");
    let db = tmp.path().join("db");
    let cas = tmp.path().join("cas");

    let mut command = Command::new(BIN);
    command.env_remove("RUN_MODE").stdin(Stdio::null());
    for (key, value) in retired_profile_env(tmp.path(), free_port()) {
        command.env(key, value);
    }
    let output = command.output().expect("the binary runs");

    assert!(
        !output.status.success(),
        "the production binary must refuse the embedded profile, not boot it"
    );
    let text = String::from_utf8_lossy(&output.stderr).into_owned();
    // The message names the retirement, the destination and the retirement of
    // the tools: whoever reads the log knows exactly where to migrate.
    for fragment in [
        "retired (HSP-213",
        "declare a [graph] section",
        "HSP-314 F4",
    ] {
        assert!(
            text.contains(fragment),
            "the refusal must name `{fragment}`; stderr was:\n{text}"
        );
    }
    assert!(
        !db.exists() && !cas.exists(),
        "the refusal must not create the local stores of the profile it refuses"
    );
}
