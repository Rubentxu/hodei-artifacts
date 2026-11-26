//! HSP-120 `dotnet nuget push` and `dotnet restore` against the
//! **real client** (`dotnet`).
//!
//! The HTTP suite measures the protocol; this one measures that the `dotnet` binary,
//! with its NuGet source pointing at the hub, uploads the package that `pack`
//! produced and then downloads it into its cache **with the exact bytes** that
//! were published. The preserved digest is not a promise of the harness: it is
//! what the client itself checks (the package's `contentHash` and
//! its `.sha512`) before accepting the download as good.
//!
//! The .NET SDK is needed: without it, the suite **fails** saying that it
//! is missing, like the other real-client suites.
//!
//! Unlike the `go` client, NuGet speaks http without blinking: no
//! TLS bridge is needed. The client cache is isolated with
//! `NUGET_PACKAGES`, and the HOME with `DOTNET_CLI_HOME`: nothing of the
//! real HOME is read or written.

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

use support::{Answer, Curl, Deployment};

/// The catalog seeder.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// The `dotnet` binary: the PATH first, and then the local installation
/// of the official script.
fn require_dotnet() -> PathBuf {
    if let Some(found) = std::env::var_os("DOTNET")
        .map(PathBuf::from)
        .filter(|path| path.exists())
    {
        return found;
    }
    let from_path = which("dotnet");
    if let Some(found) = from_path {
        return found;
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).expect("a HOME");
    let installed = home.join(".local/share/dotnet/dotnet");
    assert!(
        installed.exists(),
        "the .NET SDK is not installed: the dotnet client suite needs `dotnet` in the PATH or at {}",
        installed.display()
    );
    installed
}

fn which(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.exists())
}

fn sha256_of(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn text_of(answer: &Answer) -> String {
    String::from_utf8_lossy(&answer.body).into_owned()
}

/// The client's home: all the dotnet state inside the deployment.
struct Client {
    dotnet: PathBuf,
    root: PathBuf,
}

impl Client {
    /// `dotnet <args...>` in the given directory, with the isolated environment.
    fn run(&self, work: &Path, args: &[&str]) -> std::process::Output {
        let output = Command::new(&self.dotnet)
            .current_dir(work)
            .env("HOME", &self.root)
            .env("DOTNET_CLI_HOME", &self.root)
            .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
            .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1")
            .env("DOTNET_NOLOGO", "1")
            // No daemons inheriting descriptors: `pack` and `build`
            // spawn VBCSCompiler and MSBuild nodes that outlive the
            // child and hold on to the pipe of the step's output, so
            // that the gate transcript never sees the EOF.
            .env("DOTNET_CLI_DO_NOT_USE_MSBUILD_SERVER", "1")
            .env("MSBUILDDISABLENODEREUSE", "1")
            .env("UseSharedCompilation", "false")
            // The client's package cache, in here: the test
            // can require that the package was downloaded from the hub, and
            // not find it already in the operator's cache.
            .env("NUGET_PACKAGES", self.root.join("packages"))
            .args(args)
            .output()
            .expect("dotnet runs");
        assert!(
            output.status.success(),
            "dotnet {}: stdout: {}\nstderr: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }
}

/// The NuGet.config that makes the hub **the only source in the world**:
/// `<clear/>` is not decoration, it is the proof that what was downloaded came from
/// Hodei and not from nuget.org. The source credentials travel as
/// Basic `user:key`, with the key as the password: it is what the
/// client sends on each read request.
fn write_only_the_hub(root: &Path, port: u16, token: &str) {
    std::fs::write(
        root.join("NuGet.config"),
        format!(
            r#"<configuration>
  <packageSources>
    <clear />
    <add key="hodei" value="http://127.0.0.1:{port}/nuget/nuget-cli/v3/index.json" allowInsecureConnections="true" />
  </packageSources>
  <packageSourceCredentials>
    <hodei>
      <add key="Username" value="hodei" />
      <add key="ClearTextPassword" value="{token}" />
    </hodei>
  </packageSourceCredentials>
</configuration>
"#
        ),
    )
    .expect("the nuget config");
}

#[test]
fn the_real_dotnet_client_pushes_and_restores_with_the_digest_preserved() {
    let dotnet = require_dotnet();
    let deployment = Deployment::new();
    deployment.seed_nuget(&[("nuget-cli", serde_json::Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    let root = deployment.directory.path().join("dotnetcli");
    std::fs::create_dir_all(&root).expect("the client home");
    let client = Client { dotnet, root };

    // 1. `dotnet new classlib` + `dotnet pack`: a real package,
    //    produced by the SDK itself.
    let proj = client.root.join("src");
    std::fs::create_dir_all(&proj).expect("the project directory");
    client.run(&proj, &["new", "classlib", "-n", "CliPkg", "--force"]);
    let csproj = proj.join("CliPkg").join("CliPkg.csproj");
    let edited = std::fs::read_to_string(&csproj)
        .expect("the csproj")
        .replace(
            "</PropertyGroup>",
            "    <PackageId>Sample.Cli.Pkg</PackageId>\n    <Version>1.0.0</Version>\n    <Authors>Hodei</Authors>\n  </PropertyGroup>",
        );
    std::fs::write(&csproj, edited).expect("the csproj is rewritten");
    client.run(
        &proj.join("CliPkg"),
        &["pack", "-c", "Release", "-p:UseSharedCompilation=false"],
    );
    let nupkg = glob_nupkg(&proj.join("CliPkg").join("bin/Release"));
    let pushed_bytes = std::fs::read(&nupkg).expect("the packed nupkg");

    // 2. `dotnet nuget push`: the key travels in X-NuGet-ApiKey, as
    //    the SDK does. The source is named by config file, not
    //    by raw URL: the client only allows http when the
    //    NuGet.config itself declares it with allowInsecureConnections, and the
    //    command line cannot carry that declaration.
    write_only_the_hub(&proj, port, &token);
    client.run(
        &proj,
        &[
            "nuget",
            "push",
            nupkg.to_str().expect("a path"),
            "--source",
            "hodei",
            "--api-key",
            &token,
        ],
    );

    // The hub stores it under the normalized identity.
    let served = call(
        port,
        "GET",
        "/nuget/nuget-cli/flatcontainer/sample.cli.pkg/1.0.0/sample.cli.pkg.nupkg",
        &token,
    );
    assert_eq!(served.status, 200, "{}", text_of(&served));
    assert_eq!(
        sha256_of(&served.body),
        sha256_of(&pushed_bytes),
        "el push no guardo los bytes que fabrico pack"
    );

    // 3. The consumer: NuGet.config with the hub as the only source in the
    //    world, and `dotnet restore`.
    let consumer = client.root.join("consumer");
    std::fs::create_dir_all(&consumer).expect("the consumer directory");
    write_only_the_hub(&consumer, port, &token);
    client.run(&consumer, &["new", "console", "-n", "Consumer", "--force"]);
    let consumer_csproj = consumer.join("Consumer").join("Consumer.csproj");
    let edited = std::fs::read_to_string(&consumer_csproj)
        .expect("the consumer csproj")
        .replace(
            "</Project>",
            "  <ItemGroup>\n    <PackageReference Include=\"Sample.Cli.Pkg\" Version=\"1.0.0\" />\n  </ItemGroup>\n</Project>",
        );
    std::fs::write(&consumer_csproj, edited).expect("the consumer csproj is rewritten");
    // The restore takes the config from the parent: copy it into the project.
    write_only_the_hub(&consumer.join("Consumer"), port, &token);
    client.run(&consumer.join("Consumer"), &["restore"]);

    // 4. The client cache has the nupkg **byte for byte**: the
    //    contentHash that the client computed matches the package that
    //    pack produced, and that is the only thing that shows the hub did not
    //    rewrite anything.
    let cached = client
        .root
        .join("packages")
        .join("sample.cli.pkg")
        .join("1.0.0")
        .join("sample.cli.pkg.1.0.0.nupkg");
    assert!(
        cached.exists(),
        "el cliente no guardo el paquete bajado del hub"
    );
    let cached_bytes = std::fs::read(&cached).expect("the cached nupkg");
    assert_eq!(
        sha256_of(&cached_bytes),
        sha256_of(&pushed_bytes),
        "la cache del cliente no tiene los bytes que se publicaron"
    );

    // 5. And `dotnet build` really uses it: it compiles against the dll of the
    //    package served by the hub.
    client.run(
        &consumer.join("Consumer"),
        &[
            "build",
            "-c",
            "Release",
            "--no-restore",
            "-p:UseSharedCompilation=false",
        ],
    );
}

fn call(port: u16, method: &str, path: &str, token: &str) -> Answer {
    let into = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("answer");
    Curl::call(port, method, path, token, None, None, Some(&into)).run()
}

/// The nupkg that `pack` left in bin/Release: there is exactly one.
fn glob_nupkg(directory: &Path) -> PathBuf {
    let found: Vec<PathBuf> = std::fs::read_dir(directory)
        .expect("the pack output")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|why| why == "nupkg"))
        .collect();
    assert_eq!(found.len(), 1, "pack did not leave exactly one nupkg");
    found.into_iter().next().expect("the nupkg")
}
