//! El `dotnet nuget push` y `dotnet restore` de HSP-120 contra el
//! **cliente real** (`dotnet`).
//!
//! La suite HTTP mide el protocolo; esta mide que el binario `dotnet`,
//! con su fuente NuGet apuntando al hub, sube el paquete que `pack`
//! fabrico y lo baja despues en su cache **con los bytes exactos** que
//! se publicaron. El digest preservado no es una promesa del arnes: es
//! lo que el propio cliente comprueba (el `contentHash` del paquete y
//! su `.sha512`) antes de dar la bajada por buena.
//!
//! Hace falta el SDK de .NET: sin el, la suite **falla** diciendo que
//! falta, como las demas suites de cliente real.
//!
//! A diferencia del cliente `go`, NuGet habla http sin pestañear: no
//! hace falta puente TLS. La cache del cliente se ailla con
//! `NUGET_PACKAGES`, y el HOME con `DOTNET_CLI_HOME`: nada del HOME
//! real se lee ni se escribe.

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

use support::{Answer, Curl, Deployment};

/// El seeder del catalogo.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// El binario `dotnet`: el PATH primero, y luego la instalacion local
/// del script oficial.
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

/// La casa del cliente: todo el estado de dotnet dentro del despliegue.
struct Client {
    dotnet: PathBuf,
    root: PathBuf,
}

impl Client {
    /// `dotnet <args...>` en el directorio dado, con el entorno aislado.
    fn run(&self, work: &Path, args: &[&str]) -> std::process::Output {
        let output = Command::new(&self.dotnet)
            .current_dir(work)
            .env("HOME", &self.root)
            .env("DOTNET_CLI_HOME", &self.root)
            .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
            .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1")
            .env("DOTNET_NOLOGO", "1")
            // Nada de demonios heredando descriptores: `pack` y `build`
            // levantan VBCSCompiler y nodos de MSBuild que sobreviven al
            // hijo y se quedan con el pipe de la salida del paso, de modo
            // que el transcript del gate nunca ve el EOF.
            .env("DOTNET_CLI_DO_NOT_USE_MSBUILD_SERVER", "1")
            .env("MSBUILDDISABLENODEREUSE", "1")
            .env("UseSharedCompilation", "false")
            // La cache de paquetes del cliente, aqui dentro: la prueba
            // puede exigir que el paquete haya bajado del hub, y no
            // encontrarlo ya en la cache del operador.
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

/// El NuGet.config que hace del hub **la unica fuente del mundo**:
/// `<clear/>` no es decoracion, es la prueba de que lo bajado vino de
/// Hodei y no de nuget.org. Las credenciales de la fuente viajan como
/// Basic `usuario:clave`, con la clave como contrasena: es lo que
/// manda el cliente en cada peticion de lectura.
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

    // 1. `dotnet new classlib` + `dotnet pack`: un paquete de verdad,
    //    fabricado por el propio SDK.
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

    // 2. `dotnet nuget push`: la clave viaja en X-NuGet-ApiKey, como
    //    el SDK hace. La fuente se nombra por fichero de config, no
    //    por URL cruda: el cliente solo admite http cuando el propio
    //    NuGet.config lo declara con allowInsecureConnections, y la
    //    linea de comandos no puede llevar esa declaracion.
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

    // El hub lo guarda bajo la identidad normalizada.
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

    // 3. El consumidor: NuGet.config con el hub como unica fuente del
    //    mundo, y `dotnet restore`.
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
    // El restore toma el config del padre: copiarlo dentro del proyecto.
    write_only_the_hub(&consumer.join("Consumer"), port, &token);
    client.run(&consumer.join("Consumer"), &["restore"]);

    // 4. La cache del cliente tiene el nupkg **byte a byte**: el
    //    contentHash que el cliente calculo casa con el paquete que
    //    pack fabrico, y eso es lo unico que demuestra que el hub no
    //    reescribio nada.
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

    // 5. Y `dotnet build` lo usa de verdad: compila contra la dll del
    //    paquete servido por el hub.
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

/// El nupkg que `pack` dejo en bin/Release: hay exactamente uno.
fn glob_nupkg(directory: &Path) -> PathBuf {
    let found: Vec<PathBuf> = std::fs::read_dir(directory)
        .expect("the pack output")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|why| why == "nupkg"))
        .collect();
    assert_eq!(found.len(), 1, "pack did not leave exactly one nupkg");
    found.into_iter().next().expect("the nupkg")
}
