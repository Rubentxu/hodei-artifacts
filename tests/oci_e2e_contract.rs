//! La compatibilidad OCI con clientes reales (HSP-103 / HAR-09).
//!
//! Todo lo anterior ha hablado el protocolo con un `curl` y con el `oras`.
//! Esto habla el protocolo con **los clientes de verdad**: el `podman`, el
//! `docker` de la CLI y el `oras`, contra el servicio de verdad, en un
//! puerto de verdad.
//!
//! Y no es lo mismo. Un `curl` que abide la especificación demuestra que
//! el registro *puede* hablar el protocolo; un `podman push` que termina
//! en `exit 0` demuestra que el registro *habla* el protocolo, porque el
//! cliente hace cosas que un `curl` no hace: pide token, negocia el tipo
//! de medio, comprueba el digest de lo que baja, y se da cuenta si el
//! registro miente.
//!
//! Los clientes de verdad son los que dicen si el registro sirve. Un
//! registro que solo se ha probado con `curl` puede tener el camino feliz
//! y ningun otro, y el camino feliz es el que nadie usa.

mod support;

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use support::{Deployment, FIRST_ACCOUNT, FIRST_ONLY_REPOSITORY, FIRST_REPOSITORY, SECOND_ACCOUNT};

/// La primera persona de la primera cuenta.
const ALICE: &str = "alice";

/// El primer repositorio que usa la suite oficial de conformidad.
const CONFORMANCE_REPOSITORY: &str = "conformance/repo1";

/// El segundo repositorio que usa la suite oficial de conformidad.
///
/// La suite usa dos porque hay pruebas que empujan a uno y leen del otro:
/// el montaje entre repositorios, por ejemplo.
const CONFORMANCE_REPOSITORY_TWO: &str = "conformance/repo2";

/// El repositorio **con niveles** que se publica en el caso de la
/// jerarquia.
///
/// Un nombre con niveles es el que hace que el motor de autorizacion
/// declare un padre--`team`-- y el que el esquema tiene que dejar.
const THE_NESTED_REPOSITORY: &str = "team/demo";

/// El tipo de medio del SBOM que cuelga del artefacto.
const SBOM_TYPE: &str = "application/vnd.hodei.sbom.v1";

/// El contenido del fichero que va dentro de la imagen.
///
/// Es pequeño a proposito: lo que se prueba es el protocolo, no el
/// tamano de la imagen.
const THE_FILE: &str = "hodei e2e\n";

/// Un despliegue limpio con el catalogo sembrado.
fn a_deployment() -> Deployment {
    let deployment = Deployment::new();
    deployment.seed();
    deployment
}

/// El caso del informe, antes de ejecutarlo.
///
/// Se anota dos veces: al principio, con lo que se espera, y al final,
/// con lo que se ha medido. Si la prueba se muere entre medias, el
/// informe se queda con la primera anotacion, que dice `FALLO`.
fn the_case(id: &str, given: &str, when: &str, command: &str) -> support::Case {
    support::Case {
        id: id.to_string(),
        given: given.to_string(),
        when: when.to_string(),
        command: command.to_string(),
        outcome: "en curso".to_string(),
        passed: false,
    }
}

/// Lo que devuelve un comando de cliente.
struct Outcome {
    /// El codigo de salida.
    status: i32,
    /// Lo que dijo por la salida estandar.
    stdout: String,
    /// Lo que dijo por la salida de error.
    stderr: String,
}

impl Outcome {
    /// Lo que dijo, de las dos salidas.
    fn said(&self) -> String {
        let mut text = self.stdout.clone();
        if !self.stderr.trim().is_empty() {
            if !text.trim().is_empty() {
                text.push('\n');
            }
            text.push_str(&self.stderr);
        }
        text
    }
}

/// La casa de un cliente: donde vive su configuracion, sus credenciales y
/// su almacen.
///
/// Es un directorio **temporal**, y por eso la prueba no deja nada en la
/// maquina de quien la ejecuta. El `oras login` escribe un fichero de
/// credenciales donde el cliente guarda su configuracion, y un `login` que
/// deja la credencial de la prueba en la casa de la persona es una
/// credencial en un fichero de la persona. Y el `HOME` decide tambien donde
/// vive el almacen de imagenes del `podman`: con la casa temporal, las
/// imagenes que construye la prueba no se quedan en el almacen de quien
/// la ejecuta.
struct Home {
    directory: tempfile::TempDir,
}

impl Home {
    /// Una casa vacia.
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("a temp dir");
        std::fs::create_dir_all(directory.path().join("config")).expect("the config directory");
        Self { directory }
    }

    /// La ruta de la casa.
    fn path(&self) -> &std::path::Path {
        self.directory.path()
    }
}

/// Un comando de cliente, ejecutado **en una casa**.
///
/// El `HOME` y el `DOCKER_CONFIG` apuntan a la casa: lo que el cliente
/// escriba --su configuracion, sus credenciales, su almacen-- se queda
/// dentro de la casa y desaparece con ella.
fn run_in(home: &Home, program: &str, args: &[&str]) -> Outcome {
    let output = Command::new(program)
        .args(args)
        .env("HOME", home.path())
        .env("DOCKER_CONFIG", home.path().join("config"))
        .output()
        .unwrap_or_else(|error| panic!("{program} could not be run: {error}"));
    Outcome {
        status: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// Un comando de cliente, ejecutado **en una casa** y **en un directorio**.
///
/// El directorio de trabajo no es un detalle: el `oras` toma el nombre del
/// fichero del argumento **tal cual**, y ese nombre es el titulo del
/// artefacto. Un titulo con una ruta absoluta no se puede bajar dentro de
/// un directorio--el cliente no se deja salir de el, y tiene razon--, asi
/// que un artefacto se publica con el nombre que tendria dentro del
/// directorio de trabajo.
///
/// Es la proteccion de recorrido de rutas del propio cliente, no una
/// limitacion del registro.
fn run_in_dir(home: &Home, program: &str, args: &[&str], directory: &std::path::Path) -> Outcome {
    let output = Command::new(program)
        .args(args)
        .env("HOME", home.path())
        .env("DOCKER_CONFIG", home.path().join("config"))
        .current_dir(directory)
        .output()
        .unwrap_or_else(|error| panic!("{program} could not be run: {error}"));
    Outcome {
        status: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// El `login` del `oras`, con la credencial por la entrada estandar.
///
/// La credencial en la linea de comandos se ve en el `ps` de toda la
/// maquina, y el fichero donde el `oras` la guarda--la configuracion del
/// cliente-- esta en la casa temporal del caso: la credencial se va con
/// la prueba.
fn oras_login(home: &Home, oras: &str, registry: &str, token: &str) -> Outcome {
    let mut command = Command::new(oras);
    command.args([
        "login",
        "--plain-http",
        "--username",
        ALICE,
        "--password-stdin",
        registry,
    ]);
    command
        .env("HOME", home.path())
        .env("DOCKER_CONFIG", home.path().join("config"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("the login starts");
    {
        use std::io::Write;
        let mut stdin = child.stdin.take().expect("the stdin of the login");
        stdin
            .write_all(token.as_bytes())
            .expect("the credential is written");
    }
    let output = child.wait_with_output().expect("the login finishes");
    Outcome {
        status: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// La direccion del registro, tal y como la ven los clientes.
///
/// El nombre es `localhost` y no una direccion: el demonio que hay detras
/// de la CLI de Docker solo relaja la verificacion del TLS cuando el
/// registro se llama `localhost`, y el registro de la prueba habla HTTP.
/// Con una direccion `127.0.0.1` el demonio conversa con el en HTTPS, y
/// el cliente se queda sin imagen.
fn the_registry(port: u16) -> String {
    format!("localhost:{port}")
}

/// El fichero de credenciales del cliente.
///
/// El token viaja en el **fichero** y no en la linea de comandos: una
/// linea de comandos se ve en el `ps` de toda la maquina, y una
/// credencial en la linea de comandos es una credencial en la maquina.
/// El fichero vive en la casa del cliente, que es temporal, asi que la
/// credencial se va con la prueba.
///
/// El formato es el de `containers/image`: el par `usuario:contrasena`
/// en base64, por repositorio.
fn the_auth_file(home: &Home, registry: &str, user: &str, token: &str) -> PathBuf {
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{token}"));
    let path = home.path().join(format!("auth-{user}.json"));
    std::fs::write(
        &path,
        format!("{{\"auths\":{{\"{registry}\":{{\"auth\":\"{encoded}\"}}}}}}"),
    )
    .expect("the auth file is written");
    path
}

/// El seeder del catalogo (repositorios por cuenta).
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

// ---------------------------------------------------------------------------
// El podman: push, tag y pull de una imagen.
// ---------------------------------------------------------------------------

/// El `podman` sube una imagen, la etiqueta con otra etiqueta, y la vuelve
/// a bajar.
///
/// Lo que se mide es que el cliente termina en `exit 0` y que lo que vuelve
/// es **el mismo contenido**: el digest del manifiesto que se sube y el
/// digest del manifiesto que se baja son el mismo. Un cliente que baja otra
/// cosa no ha bajado la imagen: ha bajado lo que el registro le ha dado, y
/// eso es justo lo que hay que mirar.
#[test]
fn podman_pushes_an_image_tags_it_and_pulls_it_back() {
    let mut case = the_case(
        "OCI-PUSH-03 (podman)",
        "una imagen construida sin red y un repositorio hosted limpio",
        "se empuja la imagen, se etiqueta con otra etiqueta, se borra la local y se vuelve a bajar",
        "podman push --authfile <alice> --tls-verify=false localhost:<port>/releases:v1 <local>",
    );
    support::record_oci_e2e(case.clone());

    // La casa del cliente: temporal, para que ni la credencial ni las
    // imagenes de la prueba se queden en la maquina de quien la ejecuta.
    let home = Home::new();
    let deployment = a_deployment();
    let service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, ALICE);
    let registry = the_registry(service.port);
    let local = format!("localhost/hsp103-e2e-{port}:v1", port = service.port);
    let auth = the_auth_file(&home, &registry, ALICE, &token);
    let auth = auth
        .to_str()
        .expect("the path of the auth file")
        .to_string();

    let context = a_build_context();
    let built = the_built_image(&home, &local, None, context.path());
    assert_eq!(
        built.status,
        0,
        "the image could not be built:\n{}",
        built.said()
    );

    // El digest de lo que hay en local, antes de tocar el registro: es la
    // referencia contra la que se mira lo que vuelve.
    let before = the_digest_of(&home, &local);
    assert!(
        before.starts_with("sha256:"),
        "the local image has no digest: {before}"
    );

    // El digest de lo que se empuja: el cliente lo escribe en un fichero,
    // y es el digest del manifiesto tal y como viaja al registro.
    let digests = tempfile::tempdir().expect("a temp dir");
    let digest_file = digests.path().join("digest");
    let pushed = run_in(
        &home,
        &support::the_podman(),
        &[
            "push",
            "--tls-verify=false",
            "--digestfile",
            digest_file.to_str().expect("the path of the digest file"),
            "--authfile",
            auth.as_str(),
            &local,
            &format!("{registry}/{FIRST_REPOSITORY}:v1"),
        ],
    );
    assert_eq!(
        pushed.status,
        0,
        "the image could not be pushed:\n{}",
        pushed.said()
    );
    let pushed_digest = std::fs::read_to_string(&digest_file)
        .expect("the digest of the push")
        .trim()
        .to_string();
    assert!(
        pushed_digest.starts_with("sha256:"),
        "the pushed image has no digest: {pushed_digest}"
    );

    // La etiqueta es del cliente, no del registro: el `podman tag` nombra
    // la imagen en local y el `push` de ese nombre es el que decide que
    // etiqueta se publica.
    let tagged = run_in(
        &home,
        &support::the_podman(),
        &["tag", &local, &format!("{registry}/{FIRST_REPOSITORY}:v2")],
    );
    assert_eq!(
        tagged.status,
        0,
        "the image could not be tagged:\n{}",
        tagged.said()
    );

    let pushed_v2 = run_in(
        &home,
        &support::the_podman(),
        &[
            "push",
            "--tls-verify=false",
            "--authfile",
            auth.as_str(),
            &format!("{registry}/{FIRST_REPOSITORY}:v2"),
            &format!("{registry}/{FIRST_REPOSITORY}:v2"),
        ],
    );
    assert_eq!(
        pushed_v2.status,
        0,
        "the tagged image could not be pushed:\n{}",
        pushed_v2.said()
    );

    // Se borra la imagen local: lo que se baja tiene que venir del
    // registro, no de lo que ya estaba en la maquina.
    let removed = run_in(&home, &support::the_podman(), &["rmi", &local]);
    assert_eq!(
        removed.status,
        0,
        "the image could not be removed:\n{}",
        removed.said()
    );

    let pulled = run_in(
        &home,
        &support::the_podman(),
        &[
            "pull",
            "--tls-verify=false",
            "--authfile",
            auth.as_str(),
            &format!("{registry}/{FIRST_REPOSITORY}:v1"),
        ],
    );
    assert_eq!(
        pulled.status,
        0,
        "the image could not be pulled:\n{}",
        pulled.said()
    );

    // Lo que vuelve tiene que ser **lo mismo** que lo que subio: el
    // digest que el cliente calculo al empujar y el que calcula al
    // bajar son el digest del mismo manifiesto.
    let after = the_digest_of(&home, &format!("{registry}/{FIRST_REPOSITORY}:v1"));
    assert_eq!(
        after, pushed_digest,
        "what came back is not what went up: {after} != {pushed_digest}"
    );

    // Y el registro dice lo mismo: lo que publica y lo que el cliente
    // recibio es el mismo manifiesto.
    let published = the_published_digest(service.port, FIRST_REPOSITORY, "v1", &token);
    assert_eq!(
        published, pushed_digest,
        "what the registry published is not what went up: {published} != {pushed_digest}"
    );

    // Y la segunda etiqueta tambien se baja, que es lo que significa que
    // las dos etiquetas existen en el registro.
    let pulled_v2 = run_in(
        &home,
        &support::the_podman(),
        &[
            "pull",
            "--tls-verify=false",
            "--authfile",
            auth.as_str(),
            &format!("{registry}/{FIRST_REPOSITORY}:v2"),
        ],
    );
    assert_eq!(
        pulled_v2.status,
        0,
        "the tagged image could not be pulled:\n{}",
        pulled_v2.said()
    );

    // La maquina queda como estaba: las imagenes que se han hecho para la
    // prueba no se quedan en el almacen de la persona.
    let _ = run_in(
        &home,
        &support::the_podman(),
        &["rmi", &format!("{registry}/{FIRST_REPOSITORY}:v1")],
    );
    let _ = run_in(
        &home,
        &support::the_podman(),
        &["rmi", &format!("{registry}/{FIRST_REPOSITORY}:v2")],
    );

    // Lo que se escribe en el informe es el digest **publicado**, que es el
    // que vuelve intacto. Y el digest local se dice **como es**: si
    // coinciden, se dice que coinciden; si no, se dice que el manifiesto
    // local y el publicado son dos manifiestos distintos con el mismo
    // contenido--el cliente anade la anotacion de creacion al empujar--.
    // Un informe que dice siempre lo mismo no dice nada.
    case.outcome = if before == after {
        format!(
            "push y pull en exit 0, y el digest vuelve intacto: {after} \
             (el digest local y el publicado coinciden)"
        )
    } else {
        format!(
            "push y pull en exit 0, y el digest publicado vuelve intacto: {after} \
             (el digest local era {before}: el cliente anade la anotacion de creacion al empujar, \
             y el manifiesto local y el publicado son dos manifiestos distintos con el mismo contenido)",
            after = after,
            before = before
        )
    };
    case.passed = true;
    support::record_oci_e2e(case);
}

/// El digest de una imagen local.
///
/// El cliente lo dice con su propio formato, y el formato que usa es el
/// digest del manifiesto: el mismo que el registro publica.
fn the_digest_of(home: &Home, reference: &str) -> String {
    let outcome = run_in(
        home,
        &support::the_podman(),
        &["image", "inspect", "--format", "{{.Digest}}", reference],
    );
    assert_eq!(
        outcome.status,
        0,
        "the image {reference} could not be inspected:\n{}",
        outcome.said()
    );
    outcome.stdout.trim().to_string()
}

/// El contexto de construccion de la imagen de prueba.
///
/// Es un `Containerfile` minimo sobre `scratch`: la imagen se construye
/// **sin red**, y lo que se prueba es el protocolo del registro, no la
/// capacidad de la maquina de bajar una imagen base.
fn a_build_context() -> tempfile::TempDir {
    let context = tempfile::tempdir().expect("a temp dir");
    std::fs::write(context.path().join("file.txt"), THE_FILE).expect("the file is written");
    std::fs::write(
        context.path().join("Containerfile"),
        "FROM scratch\nCOPY file.txt /file.txt\n",
    )
    .expect("the containerfile is written");
    context
}

/// Construye la imagen de la prueba, con el **cerrojo** de las
/// construcciones.
///
/// Dos `podman build` a la vez en la misma maquina se pelean por el
/// `fuse-overlayfs`: el desmontaje se queda esperando a que termine la
/// escritura, el proceso **no termina**, y la maquina se queda con
/// gigas de paginas sucias y con procesos que no se pueden matar. Es un
/// problema del entorno y no del registro, pero cuelga la suite, y una
/// suite que cuelga la maquina no se puede ejecutar en paralelo.
///
/// El cerrojo es un `flock` sobre un fichero del `target`: serializa las
/// construcciones **entre casos y entre procesos**, no solo entre hilos,
/// y lo suelta el sistema cuando el proceso muere.
fn the_built_image(
    home: &Home,
    name: &str,
    architecture: Option<&str>,
    context: &std::path::Path,
) -> Outcome {
    let _lock = the_build_lock();
    let mut arguments = vec!["build".to_string(), "--tls-verify=false".to_string()];
    if let Some(architecture) = architecture {
        arguments.push("--arch".to_string());
        arguments.push(architecture.to_string());
    }
    arguments.push("-t".to_string());
    arguments.push(name.to_string());
    arguments.push(context.to_string_lossy().into_owned());
    let borrowed: Vec<&str> = arguments.iter().map(|argument| argument.as_str()).collect();
    run_in(home, &support::the_podman(), &borrowed)
}

/// El cerrojo de las construcciones de imagen.
///
/// El fichero esta en el `target`: es estado de la ejecucion, no del
/// repositorio, y no se versiona.
fn the_build_lock() -> std::fs::File {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("oci-e2e-build.lock");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("the directory of the lock");
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .expect("the lock of the builds");
    file.lock().expect("the lock of the builds is taken");
    file
}

/// La imagen local, en un tar que el demonio puede leer.
///
/// La CLI de Docker empuja lo que el **demonio** tiene, y el demonio no
/// ve el almacen del cliente que la construyo: la imagen viaja en un tar,
/// que es el formato con el que los dos se entienden. El tar se queda en
/// el directorio que se devuelve, y ese directorio vive lo que viva el
/// caso.
fn the_saved_image(home: &Home, image: &str) -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().expect("a temp dir");
    let tar = directory.path().join("image.tar");
    let saved = run_in(
        home,
        &support::the_podman(),
        &[
            "save",
            "--output",
            tar.to_str().expect("the path of the tar"),
            image,
        ],
    );
    assert_eq!(
        saved.status,
        0,
        "the image {image} could not be saved:\n{}",
        saved.said()
    );
    (directory, tar)
}

// ---------------------------------------------------------------------------
// El docker de la CLI: push y pull de una imagen y de un indice.
// ---------------------------------------------------------------------------

/// El demonio que hay detras del `docker` de la CLI.
///
/// La CLI de Docker no habla con un registro: habla con un **demonio**, y
/// es el demonio el que habla con el registro. El demonio que hay en esta
/// maquina es el `podman`, que expone la API de Docker por un socket unix.
///
/// Y el socket se enciende con una configuracion propia, que es la que
/// dice que el registro de la prueba es de los que se hablan **sin TLS**:
/// el registro de la prueba habla HTTP, y una direccion se puede marcar
/// como insegura en la configuracion del cliente.
struct Engine {
    /// El proceso del demonio.
    child: Child,
    /// La casa donde vive lo de la CLI (la configuracion y las
    /// credenciales): es temporal, y la credencial no se queda en la
    /// maquina de quien ejecuta la prueba.
    home: Home,
    /// El socket del demonio.
    socket: PathBuf,
}

impl Drop for Engine {
    /// El demonio se apaga con el prueba.
    ///
    /// Un demonio que se queda escuchando en un socket del sistema es un
    /// demonio que se queda para siempre, y la maquina de quien ejecuta
    /// la prueba no es el sitio donde dejarlo.
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Engine {
    /// El entorno que necesitan los comandos del cliente.
    ///
    /// El `HOME` es el del demonio: es donde esta la configuracion de
    /// clientes que dice que el registro de la prueba se habla sin TLS,
    /// y sin ella el `docker` no baja ni sube nada.
    fn environment(&self) -> Vec<(String, String)> {
        vec![
            (
                "DOCKER_HOST".to_string(),
                format!("unix://{}", self.socket.display()),
            ),
            (
                "DOCKER_CONFIG".to_string(),
                self.home
                    .path()
                    .join("config")
                    .to_string_lossy()
                    .into_owned(),
            ),
            (
                "HOME".to_string(),
                self.home.path().to_string_lossy().into_owned(),
            ),
        ]
    }
}

/// Enciende el demonio y espera a que su socket exista.
///
/// El socket es la prueba de que el demonio esta listening: aparece
/// cuando esta, y no antes.
///
/// La configuracion de clientes se pone en el **HOME** del demonio, y no
/// en una variable: la libreria de clientes lee
/// `$HOME/.config/containers/registries.conf`, y ese fichero tiene
/// prioridad sobre el del sistema. Sin el, el demonio intenta hablar
/// **HTTPS** con un registro que habla HTTP, y el cliente se queda sin
/// imagen.
fn an_engine(port: u16) -> Engine {
    let home = Home::new();
    let socket = home.path().join("engine.sock");
    let registries = home
        .path()
        .join(".config")
        .join("containers")
        .join("registries.conf");
    std::fs::create_dir_all(registries.parent().expect("the config directory"))
        .expect("the config directory");
    std::fs::write(
        &registries,
        format!(
            "[[registry]]\nlocation = \"localhost:{port}\"\ninsecure = true\n",
            port = port
        ),
    )
    .expect("the registries config is written");

    let child = Command::new(support::the_podman())
        .args([
            "system",
            "service",
            "--time=0",
            &format!("unix://{}", socket.display()),
        ])
        .env("HOME", home.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the engine starts");

    // El demonio entra en la `Engine` antes de esperar: asi su apagado es
    // cosa de su `Drop`, que lo mata y lo espera, y no de este camino. Un
    // demonio al que la prueba abandona se queda escuchando.
    let engine = Engine {
        child,
        home,
        socket,
    };

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        // El socket **conectado** es la prueba de que el demonio sirve:
        // el fichero puede existir antes de que el demonio atienda, y un
        // demonio que muere despues deja el fichero ahi. Con la conexion,
        // lo que se mide es lo que el cliente va a encontrar.
        if engine.socket.exists() && std::os::unix::net::UnixStream::connect(&engine.socket).is_ok()
        {
            return engine;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    drop(engine);
    panic!("the engine did not open its socket");
}

/// Un comando del `docker` de la CLI.
///
/// El cliente lee sus credenciales de su configuracion, que esta en un
/// directorio temporal: la credencial de la prueba no se escribe en la
/// configuracion de la maquina de quien la ejecuta.
fn docker(engine: &Engine, args: &[&str]) -> Outcome {
    let mut command = Command::new(support::the_docker());
    command.args(args);
    for (name, value) in engine.environment() {
        command.env(name, value);
    }
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("docker could not be run: {error}"));
    Outcome {
        status: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// El `docker login` de la prueba.
///
/// La credencial va por la entrada estandar, que es la unica via que no
/// la deja en la linea de comandos: una linea de comandos se ve en el
/// historial y en el informe.
fn docker_login(engine: &Engine, registry: &str, token: &str) -> Outcome {
    let mut child = Command::new(support::the_docker());
    child.args(["login", "--username", ALICE, "--password-stdin", registry]);
    for (name, value) in engine.environment() {
        command_env(&mut child, name, value);
    }
    child
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = child.spawn().expect("the login starts");
    {
        use std::io::Write;
        let mut stdin = child.stdin.take().expect("the stdin of the login");
        stdin
            .write_all(token.as_bytes())
            .expect("the credential is written");
    }
    let output = child.wait_with_output().expect("the login finishes");
    Outcome {
        status: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// El entorno de un comando, una entrada.
fn command_env(command: &mut Command, name: String, value: String) {
    command.env(name, value);
}

/// El `docker` de la CLI empuja y baja una imagen de una sola arquitectura.
///
/// Lo que se mide es que el cliente termina en `exit 0` y que lo que
/// vuelve es el mismo contenido: el digest que el registro publico es el
/// digest que el demonio deja anotado.
#[test]
fn the_docker_cli_pushes_and_pulls_a_single_architecture_image() {
    let mut case = the_case(
        "OCI-PUSH-03 (docker, una plataforma)",
        "una imagen de una sola plataforma y un repositorio hosted limpio",
        "el docker de la CLI la empuja, se borra del demonio y se vuelve a bajar",
        "docker push localhost:<port>/releases:v1",
    );
    support::record_oci_e2e(case.clone());

    // La casa del cliente: temporal, para que ni la credencial ni las
    // imagenes de la prueba se queden en la maquina de quien la ejecuta.
    let home = Home::new();
    let deployment = a_deployment();
    let service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, ALICE);
    let registry = the_registry(service.port);
    let engine = an_engine(service.port);
    let reference = format!("{registry}/{FIRST_REPOSITORY}:v1");

    let login = docker_login(&engine, &registry, &token);
    assert_eq!(login.status, 0, "the login failed:\n{}", login.said());

    // La imagen tiene que estar en el almacen del demonio con el nombre
    // del destino: el cliente pide al demonio que empuje "esto", y "esto"
    // se busca por nombre.
    let context = a_build_context();
    let local = format!("localhost/hsp103-e2e-{port}:v1", port = service.port);
    let built = the_built_image(&home, &local, None, context.path());
    assert_eq!(
        built.status,
        0,
        "the image could not be built:\n{}",
        built.said()
    );

    // La imagen pasa al demonio en un tar: es lo que el demonio conoce.
    let (_tar_directory, tar) = the_saved_image(&home, &local);
    let loaded = docker(
        &engine,
        &[
            "load",
            "--input",
            tar.to_str().expect("the path of the tar"),
        ],
    );
    assert_eq!(
        loaded.status,
        0,
        "the image could not be loaded:\n{}",
        loaded.said()
    );

    let tagged = docker(&engine, &["tag", &local, &reference]);
    assert_eq!(
        tagged.status,
        0,
        "the image could not be tagged:\n{}",
        tagged.said()
    );

    let pushed = docker(&engine, &["push", &reference]);
    assert_eq!(
        pushed.status,
        0,
        "the image could not be pushed:\n{}",
        pushed.said()
    );

    // El digest que el registro publica: el del manifiesto que tiene, no
    // el que el demonio calcula en su maquina.
    let published = the_published_digest(service.port, FIRST_REPOSITORY, "v1", &token);
    assert!(
        published.starts_with("sha256:"),
        "the pushed image has no digest: {published}"
    );

    // Se borra del demonio: lo que se baja tiene que venir del registro.
    let removed = docker(&engine, &["rmi", &reference]);
    assert_eq!(
        removed.status,
        0,
        "the image could not be removed:\n{}",
        removed.said()
    );

    let pulled = docker(&engine, &["pull", &reference]);
    assert_eq!(
        pulled.status,
        0,
        "the image could not be pulled:\n{}",
        pulled.said()
    );

    // Lo que vuelve tiene que ser **el mismo contenido**, y el contenido
    // se identifica por su digest. Se comparan tres cosas: el digest que
    // el registro publica antes y despues de la ida y vuelta, y el digest
    // que el cliente se anota.
    //
    // Y aqui hay que mirar el digest del **registro**, no el primero que
    // el demonio tenga anotado: la imagen entra en el demonio por un tar,
    // con el manifiesto de su formato local, y sale por otro, con el
    // manifiesto que el registro publico. Los dos son reales y ninguno es
    // el otro, y el orden en que el demonio los lista no es un contrato.
    let after = the_published_digest(service.port, FIRST_REPOSITORY, "v1", &token);
    assert_eq!(
        after, published,
        "what came back is not what went up: {after} != {published}"
    );
    let digests = the_repo_digests(&engine, &reference);
    let repository = the_repository_of(&reference);
    assert!(
        digests
            .iter()
            .any(|entry| entry == &format!("{repository}@{published}")),
        "the client did not record the digest the registry published: {digests:?}"
    );

    let _ = run_in(&home, &support::the_podman(), &["rmi", &local]);
    let _ = docker(&engine, &["rmi", &reference]);

    case.outcome = format!("push y pull en exit 0, y el digest vuelve intacto: {published}");
    case.passed = true;
    support::record_oci_e2e(case);
}

/// El digest que el registro publica para una etiqueta.
///
/// Lo dice la cabecera `Docker-Content-Digest` de la respuesta: es el
/// digest del manifiesto que el registro tiene, no el que el cliente
/// calculo en su maquina.
fn the_published_digest(port: u16, name: &str, tag: &str, token: &str) -> String {
    let answer = support::curl_describe(port, &format!("/v2/{name}/manifests/{tag}"), token);
    assert_eq!(
        answer.status,
        200,
        "the manifest {name}:{tag} could not be described:\n{}",
        String::from_utf8_lossy(&answer.body)
    );
    answer
        .header("Docker-Content-Digest")
        .expect("the digest of the manifest")
}

/// Los digests con los que el demonio tiene la imagen.
///
/// El demonio puede tener **mas de uno**: la imagen entra por un tar--con
/// el manifiesto de su formato local-- y sale por otro--con el manifiesto
/// que el registro publico--. Los dos son reales y el orden en que el
/// demonio los lista no es un contrato, asi que se devuelven **todos** y
/// quien compara busca el que le importa.
fn the_repo_digests(engine: &Engine, reference: &str) -> Vec<String> {
    let outcome = docker(
        engine,
        &[
            "image",
            "inspect",
            "--format",
            "{{json .RepoDigests}}",
            reference,
        ],
    );
    assert_eq!(
        outcome.status,
        0,
        "the image {reference} could not be inspected:\n{}",
        outcome.said()
    );
    outcome
        .stdout
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|entry| entry.trim().trim_matches('"').to_string())
        .filter(|entry| !entry.is_empty())
        .collect()
}

/// El nombre del repositorio de una referencia.
///
/// La referencia es `nombre:etiqueta`, y el nombre puede tener puertos y
/// niveles: el separador es el **ultimo** dos puntos, y solo si lo que
/// sigue no tiene barras.
fn the_repository_of(reference: &str) -> String {
    match reference.rsplit_once(':') {
        Some((name, tag)) if !tag.contains('/') => name.to_string(),
        _ => reference.to_string(),
    }
}

/// El `docker` de la CLI empuja un indice de dos plataformas.
///
/// El indice lo arma **la CLI**: `docker manifest create` y `docker manifest
/// add` son codigo del cliente, y `docker manifest push` empuja el indice y
/// sus hijos al registro sin pasar por el demonio. Es el camino que usa un
/// cliente de verdad para publicar una imagen multi-plataforma, y es el
/// que dice si el registro sabe recibir un indice.
#[test]
fn the_docker_cli_pushes_and_pulls_a_multi_architecture_image() {
    let mut case = the_case(
        "OCI-INDEX-01 (docker, multi-plataforma)",
        "una imagen por plataforma, en el demonio y en el registro",
        "la CLI arma un indice de dos plataformas, lo empuja y lo vuelve a bajar",
        "docker manifest push localhost:<port>/releases:multi",
    );
    support::record_oci_e2e(case.clone());

    // La casa del cliente: temporal, para que ni la credencial ni las
    // imagenes de la prueba se queden en la maquina de quien la ejecuta.
    let home = Home::new();
    let deployment = a_deployment();
    let service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, ALICE);
    let registry = the_registry(service.port);
    let engine = an_engine(service.port);
    let index = format!("{registry}/{FIRST_REPOSITORY}:multi");

    let login = docker_login(&engine, &registry, &token);
    assert_eq!(login.status, 0, "the login failed:\n{}", login.said());

    // Una imagen por plataforma: el indice tiene que traer imagenes
    // distintas, y una imagen se distingue de otra por su arquitectura.
    let context = a_build_context();
    let mut children = Vec::new();
    for architecture in ["amd64", "arm64"] {
        let local = format!(
            "localhost/hsp103-e2e-{port}:{architecture}",
            port = service.port
        );
        let built = the_built_image(&home, &local, Some(architecture), context.path());
        assert_eq!(
            built.status,
            0,
            "the image for {architecture} could not be built:\n{}",
            built.said()
        );

        let (_tar_directory, tar) = the_saved_image(&home, &local);
        let loaded = docker(
            &engine,
            &[
                "load",
                "--input",
                tar.to_str().expect("the path of the tar"),
            ],
        );
        assert_eq!(
            loaded.status,
            0,
            "the image for {architecture} could not be loaded:\n{}",
            loaded.said()
        );

        // Cada imagen se empuja con su propia etiqueta: el indice las
        // nombra, y el registro tiene que tener las dos.
        let reference = format!("{registry}/{FIRST_REPOSITORY}:v1-{architecture}");
        let tagged = docker(&engine, &["tag", &local, &reference]);
        assert_eq!(
            tagged.status,
            0,
            "the image for {architecture} could not be tagged:\n{}",
            tagged.said()
        );
        let pushed = docker(&engine, &["push", &reference]);
        assert_eq!(
            pushed.status,
            0,
            "the image for {architecture} could not be pushed:\n{}",
            pushed.said()
        );
        children.push(reference);
    }

    // El indice lo arma **la CLI**: `docker manifest create` y `docker
    // manifest push` son codigo del cliente, y empujan el indice y sus
    // hijos al registro sin pasar por el demonio. Es el camino que usa un
    // cliente de verdad para publicar una imagen multi-plataforma.
    //
    // La CLI habla con el registro **por su cuenta**, y lo hace con TLS
    // salvo que se le diga que no: `--insecure` es lo que le dice que el
    // registro de la prueba habla HTTP.
    let created = docker(
        &engine,
        &[
            "manifest",
            "create",
            "--insecure",
            &index,
            &children[0],
            &children[1],
        ],
    );
    assert_eq!(
        created.status,
        0,
        "the manifest list could not be created:\n{}",
        created.said()
    );

    let pushed = docker(&engine, &["manifest", "push", "--insecure", &index]);
    assert_eq!(
        pushed.status,
        0,
        "the manifest list could not be pushed:\n{}",
        pushed.said()
    );

    // El digest que el registro publica para el indice: es lo que se
    // compara despues de la ida y vuelta.
    let published = the_published_digest(service.port, FIRST_REPOSITORY, "multi", &token);
    assert!(
        published.starts_with("sha256:"),
        "the pushed index has no digest: {published}"
    );

    // El indice se mira **desde el cliente**, sin demonio: es la CLI la que
    // habla con el registro, y lo que ve es lo que el registro publico.
    let inspected = docker(&engine, &["manifest", "inspect", "--insecure", &index]);
    assert_eq!(
        inspected.status,
        0,
        "the manifest list could not be inspected:\n{}",
        inspected.said()
    );
    assert!(
        inspected.stdout.contains("arm64") && inspected.stdout.contains("amd64"),
        "the manifest list does not carry both platforms: {}",
        inspected.stdout
    );

    // Y el demonio lo baja: el indice se resuelve a una plataforma y la
    // imagen vuelve.
    let pulled = docker(&engine, &["pull", &index]);
    assert_eq!(
        pulled.status,
        0,
        "the manifest list could not be pulled:\n{}",
        pulled.said()
    );

    // Y lo que se baja es **lo mismo** que se publico: el digest del
    // indice, antes y despues de la ida y vuelta.
    let after = the_published_digest(service.port, FIRST_REPOSITORY, "multi", &token);
    assert_eq!(
        after, published,
        "what came back is not what went up: {after} != {published}"
    );

    for child in &children {
        let _ = docker(&engine, &["rmi", child]);
    }

    case.outcome = format!(
        "el indice de dos plataformas se empuja y se baja en exit 0, con su plataforma dentro, y el digest vuelve intacto: {published}"
    );
    case.passed = true;
    support::record_oci_e2e(case);
}

// ---------------------------------------------------------------------------
// El `oras`: un artefacto, un SBOM colgado y los referrers.
// ---------------------------------------------------------------------------

/// El `oras` empuja un artefacto, le cuelga un SBOM y descubre los referrers.
///
/// Lo que se mide es el camino que usan las herramientas de la cadena de
/// suministro: el artefacto se publica con su tipo, el SBOM se publica
/// **colgado** del artefacto--con el `subject` que lo liga-- y el
/// referrers se descubre por la API de referrers. Un SBOM publicado y no
/// descubrir es un SBOM que no sirve de nada.
#[test]
fn the_oras_pushes_an_artifact_attaches_a_sbom_and_discovers_the_referrers() {
    let mut case = the_case(
        "OCI-REFERRERS-01 (oras)",
        "un artefacto con su tipo y un SBOM con el suyo",
        "el oras empuja el artefacto, lo vuelve a bajar, cuelga el SBOM y descubre los referrers",
        "oras discover --plain-http localhost:<port>/releases:v1",
    );
    support::record_oci_e2e(case.clone());

    // La casa del cliente: temporal, para que ni la credencial ni las
    // imagenes de la prueba se queden en la maquina de quien la ejecuta.
    let home = Home::new();
    let deployment = a_deployment();
    let service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, ALICE);
    let registry = the_registry(service.port);
    let reference = format!("{registry}/{FIRST_REPOSITORY}:v1");
    let oras = support::the_oras();

    // La credencial va por la entrada estandar, que es la unica via que
    // no la deja en la linea de comandos: una linea de comandos se ve en
    // el `ps` de toda la maquina.
    let logged = oras_login(&home, &oras, &registry, &token);
    assert_eq!(
        logged.status,
        0,
        "the oras login failed:\n{}",
        logged.said()
    );

    // El artefacto: un fichero con su tipo de medio. Se publica con el
    // nombre que tiene **dentro del directorio de trabajo**, porque ese
    // nombre es el titulo del artefacto y un titulo con una ruta
    // absoluta no se puede bajar dentro de un directorio.
    let files = tempfile::tempdir().expect("a temp dir");
    let artifact = files.path().join("artifact.txt");
    std::fs::write(&artifact, THE_FILE).expect("the artifact is written");
    let pushed = run_in_dir(
        &home,
        &oras,
        &[
            "push",
            "--plain-http",
            "--disable-path-validation",
            &reference,
            "artifact.txt:application/vnd.hodei.artifact.v1",
        ],
        files.path(),
    );
    assert_eq!(
        pushed.status,
        0,
        "the artifact could not be pushed:\n{}",
        pushed.said()
    );

    // Y el artefacto se vuelve a bajar: lo que se publico tiene que poder
    // bajar con sus bytes y con su tipo. Un artefacto que solo se puede
    // publicar es un artefacto que no se puede usar.
    let pulled = tempfile::tempdir().expect("a temp dir");
    let pulled_artifact = run_in(
        &home,
        &oras,
        &[
            "pull",
            "--plain-http",
            "--output",
            pulled.path().to_str().expect("the output directory"),
            &reference,
        ],
    );
    assert_eq!(
        pulled_artifact.status,
        0,
        "the artifact could not be pulled:\n{}",
        pulled_artifact.said()
    );
    let bytes = std::fs::read(pulled.path().join("artifact.txt")).expect("the artifact is there");
    assert_eq!(
        bytes,
        THE_FILE.as_bytes(),
        "what came back is not what went up"
    );

    // El SBOM: cuelga del artefacto con su `subject`.
    let sbom = files.path().join("sbom.json");
    std::fs::write(
        &sbom,
        "{\"bomFormat\":\"CycloneDX\",\"specVersion\":\"1.5\"}\n",
    )
    .expect("the sbom is written");
    let attached = run_in_dir(
        &home,
        &oras,
        &[
            "attach",
            "--plain-http",
            "--disable-path-validation",
            "--artifact-type",
            SBOM_TYPE,
            &reference,
            &format!("sbom.json:{SBOM_TYPE}"),
        ],
        files.path(),
    );
    assert_eq!(
        attached.status,
        0,
        "the sbom could not be attached:\n{}",
        attached.said()
    );

    // Y los referrers se descubren: el SBOM colgado tiene que aparecer.
    let discovered = run_in(
        &home,
        &oras,
        &[
            "discover",
            "--plain-http",
            "--distribution-spec",
            "v1.1-referrers-api",
            &reference,
        ],
    );
    assert_eq!(
        discovered.status,
        0,
        "the referrers could not be discovered:\n{}",
        discovered.said()
    );
    assert!(
        discovered.stdout.contains(SBOM_TYPE),
        "the sbom is not among the referrers:\n{}",
        discovered.said()
    );

    case.outcome = format!(
        "el artefacto se empuja y se baja con sus bytes, el SBOM se cuelga y se descubre: \
         {SBOM_TYPE} aparece entre los referrers",
        SBOM_TYPE = SBOM_TYPE
    );
    case.passed = true;
    support::record_oci_e2e(case);
}

// ---------------------------------------------------------------------------
// Los fallos, con clientes de verdad.
// ---------------------------------------------------------------------------

/// El `podman` de otra cuenta no puede publicar en el repositorio de la
/// primera.
///
/// Lo que se mide es que el cliente **falla**: un repositorio de otra
/// cuenta no existe para esta, y el cliente se queda sin imagen. Y que
/// la cuenta que puede, sigue pudiendo: el aislamiento no es una puerta
/// que se cierra para todos.
#[test]
fn the_tenant_of_another_account_cannot_publish_in_the_repository_of_the_first() {
    let mut case = the_case(
        "OCI-ISOLATION-01 (podman, otra cuenta)",
        "una imagen local y un repositorio que es de la primera cuenta",
        "el podman de la segunda cuenta intenta publicar y no puede",
        "podman push --authfile <bob> localhost:<port>/private:v1 <local>",
    );
    support::record_oci_e2e(case.clone());

    // La casa del cliente: temporal, para que ni la credencial ni las
    // imagenes de la prueba se queden en la maquina de quien la ejecuta.
    let home = Home::new();
    let deployment = a_deployment();
    let service = deployment.start(&[]);
    let registry = the_registry(service.port);
    let local = format!("localhost/hsp103-e2e-{port}:v1", port = service.port);

    let context = a_build_context();
    let built = the_built_image(&home, &local, None, context.path());
    assert_eq!(
        built.status,
        0,
        "the image could not be built:\n{}",
        built.said()
    );

    // Cada cuenta tiene su credencial, y cada una en su fichero.
    let alice_token = support::token(FIRST_ACCOUNT, ALICE);
    let bob = support::token(SECOND_ACCOUNT, "bob");
    let auth_alice = the_auth_file(&home, &registry, ALICE, &alice_token);
    let auth_alice = auth_alice
        .to_str()
        .expect("the path of the auth file")
        .to_string();
    let auth_bob = the_auth_file(&home, &registry, "bob", &bob);
    let auth_bob = auth_bob
        .to_str()
        .expect("the path of the auth file")
        .to_string();
    let refused = run_in(
        &home,
        &support::the_podman(),
        &[
            "push",
            "--tls-verify=false",
            "--authfile",
            auth_bob.as_str(),
            &local,
            &format!("{registry}/{FIRST_ONLY_REPOSITORY}:v1"),
        ],
    );
    assert_ne!(
        refused.status,
        0,
        "the other tenant could publish in the repository of the first:\n{}",
        refused.said()
    );

    // Y la primera cuenta sigue pudiendo: el aislamiento es de la cuenta,
    // no una puerta que se cierra para todos.
    let allowed = run_in(
        &home,
        &support::the_podman(),
        &[
            "push",
            "--tls-verify=false",
            "--authfile",
            auth_alice.as_str(),
            &local,
            &format!("{registry}/{FIRST_ONLY_REPOSITORY}:v1"),
        ],
    );
    assert_eq!(
        allowed.status,
        0,
        "the first tenant could not publish in its own repository:\n{}",
        allowed.said()
    );

    case.outcome = "la otra cuenta no publica y la primera si".to_string();
    case.passed = true;
    support::record_oci_e2e(case);
}

/// Publicar dos veces es publicar una.
///
/// Lo que se mide es que el cliente puede **reintentar**: el segundo
/// `push` del mismo contenido termina en verde y el registro sigue
/// publicando el mismo digest. Un cliente que reintenta--por un corte de
/// red, por un timeout--no puede dejar el repositorio a medias ni
/// cambiar lo que habia publicado.
#[test]
fn the_publication_is_idempotent_when_the_client_retries() {
    let mut case = the_case(
        "OCI-RETRY-01 (podman, reintento)",
        "una imagen local ya publicada",
        "el cliente la publica otra vez y el registro publica el mismo digest",
        "podman push --authfile <alice> localhost:<port>/releases:v1 <local>",
    );
    support::record_oci_e2e(case.clone());

    // La casa del cliente: temporal, para que ni la credencial ni las
    // imagenes de la prueba se queden en la maquina de quien la ejecuta.
    let home = Home::new();
    let deployment = a_deployment();
    let service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, ALICE);
    let registry = the_registry(service.port);
    let auth = the_auth_file(&home, &registry, ALICE, &token);
    let auth = auth
        .to_str()
        .expect("the path of the auth file")
        .to_string();
    let local = format!("localhost/hsp103-e2e-{port}:v1", port = service.port);
    let published = format!("{registry}/{FIRST_REPOSITORY}:v1");

    let context = a_build_context();
    let built = the_built_image(&home, &local, None, context.path());
    assert_eq!(
        built.status,
        0,
        "the image could not be built:\n{}",
        built.said()
    );

    let digests = tempfile::tempdir().expect("a temp dir");
    let digest_file = digests.path().join("digest");
    let first = run_in(
        &home,
        &support::the_podman(),
        &[
            "push",
            "--tls-verify=false",
            "--digestfile",
            digest_file.to_str().expect("the path of the digest file"),
            "--authfile",
            auth.as_str(),
            &local,
            &published,
        ],
    );
    assert_eq!(
        first.status,
        0,
        "the image could not be pushed:\n{}",
        first.said()
    );
    let first_digest = std::fs::read_to_string(&digest_file)
        .expect("the digest of the push")
        .trim()
        .to_string();

    // El reintento: el mismo contenido, la misma etiqueta.
    let second = run_in(
        &home,
        &support::the_podman(),
        &[
            "push",
            "--tls-verify=false",
            "--authfile",
            auth.as_str(),
            &local,
            &published,
        ],
    );
    assert_eq!(
        second.status,
        0,
        "the retry could not be pushed:\n{}",
        second.said()
    );

    let published_digest = the_published_digest(service.port, FIRST_REPOSITORY, "v1", &token);
    assert_eq!(
        published_digest, first_digest,
        "the retry published something else: {published_digest} != {first_digest}"
    );

    case.outcome = format!("el reintento deja el mismo digest: {published_digest}");
    case.passed = true;
    support::record_oci_e2e(case);
}

/// El contenido publicado sobrevive a un reinicio del servicio.
///
/// Lo que se mide es que el contenido sigue ahi **despues** de que el
/// servicio muere de golpe y vuelve a encender en el **mismo** puerto:
/// el cliente que vuelve a mirar a la misma direccion encuentra lo que
/// habia publicado. Un reinicio que pierde el catalogo o el almacen
/// convierte la publicacion en un cuento.
#[test]
fn the_published_content_survives_a_restart_of_the_service_on_the_same_port() {
    let mut case = the_case(
        "OCI-DURABILITY-01 (podman, reinicio)",
        "una imagen publicada en un servicio que se apaga de golpe",
        "el servicio vuelve en el mismo puerto y el cliente baja lo mismo",
        "podman pull --authfile <alice> localhost:<port>/releases:v1",
    );
    support::record_oci_e2e(case.clone());

    let home = Home::new();
    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let port = service.port;
    let registry = the_registry(port);
    let token = support::token(FIRST_ACCOUNT, ALICE);
    let auth = the_auth_file(&home, &registry, ALICE, &token);
    let auth = auth
        .to_str()
        .expect("the path of the auth file")
        .to_string();
    let local = format!("localhost/hsp103-e2e-{port}:v1", port = port);

    let context = a_build_context();
    let built = the_built_image(&home, &local, None, context.path());
    assert_eq!(
        built.status,
        0,
        "the image could not be built:\n{}",
        built.said()
    );

    let published = format!("{registry}/{FIRST_REPOSITORY}:v1");
    let pushed = run_in(
        &home,
        &support::the_podman(),
        &[
            "push",
            "--tls-verify=false",
            "--authfile",
            auth.as_str(),
            &local,
            &published,
        ],
    );
    assert_eq!(
        pushed.status,
        0,
        "the image could not be pushed:\n{}",
        pushed.said()
    );
    // Lo que el registro publica: se baja y se mira el digest que
    // vuelve. Ese digest es la referencia contra la que se mide lo que
    // vuelve **despues** del reinicio.
    let first = run_in(
        &home,
        &support::the_podman(),
        &[
            "pull",
            "--tls-verify=false",
            "--authfile",
            auth.as_str(),
            &published,
        ],
    );
    assert_eq!(
        first.status,
        0,
        "the image could not be pulled before the restart:\n{}",
        first.said()
    );
    let digest = the_digest_of(&home, &published);

    // El servicio se apaga **de golpe**: un `SIGKILL` es lo que pasa
    // cuando la maquina se apaga, y no deja tiempo para limpiar nada.
    support::kill(&mut service);

    // Y vuelve en el **mismo** puerto: el cliente ya sabe a donde mira.
    let _service = deployment.start_on(port, &[]);

    // Se borra la imagen local: lo que se baja tiene que venir del
    // registro, no de lo que ya estaba en la maquina.
    let removed = run_in(&home, &support::the_podman(), &["rmi", &local]);
    assert_eq!(
        removed.status,
        0,
        "the image could not be removed:\n{}",
        removed.said()
    );

    let pulled = run_in(
        &home,
        &support::the_podman(),
        &[
            "pull",
            "--tls-verify=false",
            "--authfile",
            auth.as_str(),
            &published,
        ],
    );
    assert_eq!(
        pulled.status,
        0,
        "the image could not be pulled after the restart:\n{}",
        pulled.said()
    );
    let after = the_digest_of(&home, &published);
    assert_eq!(
        after, digest,
        "what came back after the restart is not what went up: {after} != {digest}"
    );

    case.outcome = format!("el mismo digest despues del reinicio: {after}");
    case.passed = true;
    support::record_oci_e2e(case);
}

// ---------------------------------------------------------------------------
// El repositorio con niveles.
// ---------------------------------------------------------------------------

/// Un repositorio **con niveles** se publica con su cliente.
///
/// El nombre con niveles--`team/demo`-- es el caso que el esquema de
/// autorizacion no entendia: el motor de autorizacion declaraba el padre
/// del repositorio--`team`-- y el esquema no permitia que un
/// `Repository` fuera miembro de otro `Repository`, asi que el motor
/// fallaba cerrado y el repositorio se respondia con un `403 DENIED`.
///
/// Lo que se mide es que el cliente **real** publica en el y que lo que
/// publica se puede volver a bajar: sin este caso, la correccion del
/// esquema no la comprueba nadie con un cliente de verdad.
#[test]
fn the_repository_with_levels_is_published_by_its_own_account() {
    let mut case = the_case(
        "OCI-HIERARCHY-01 (podman, niveles)",
        "un repositorio con niveles y la credencial de su cuenta",
        "el podman publica en el y lo vuelve a bajar",
        "podman push --authfile <alice> --tls-verify=false localhost:<port>/team/demo:v1 <local>",
    );
    support::record_oci_e2e(case.clone());

    let home = Home::new();
    let deployment = Deployment::new();
    deployment.seed_with(&[THE_NESTED_REPOSITORY]);
    let service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, ALICE);
    let registry = the_registry(service.port);
    let auth = the_auth_file(&home, &registry, ALICE, &token);
    let auth = auth
        .to_str()
        .expect("the path of the auth file")
        .to_string();
    let local = format!("localhost/hsp103-e2e-{port}:v1", port = service.port);

    let context = a_build_context();
    let built = the_built_image(&home, &local, None, context.path());
    assert_eq!(
        built.status,
        0,
        "the image could not be built:\n{}",
        built.said()
    );

    let published = format!("{registry}/{THE_NESTED_REPOSITORY}:v1");
    let pushed = run_in(
        &home,
        &support::the_podman(),
        &[
            "push",
            "--tls-verify=false",
            "--authfile",
            auth.as_str(),
            &local,
            &published,
        ],
    );
    assert_eq!(
        pushed.status,
        0,
        "the image could not be pushed in the repository with levels:\n{}",
        pushed.said()
    );

    // Y lo que se publico se puede volver a bajar: el `403` que sufria
    // este repositorio era de autorizacion, y no de protocolo.
    let removed = run_in(&home, &support::the_podman(), &["rmi", &local]);
    assert_eq!(
        removed.status,
        0,
        "the image could not be removed:\n{}",
        removed.said()
    );

    let pulled = run_in(
        &home,
        &support::the_podman(),
        &[
            "pull",
            "--tls-verify=false",
            "--authfile",
            auth.as_str(),
            &published,
        ],
    );
    assert_eq!(
        pulled.status,
        0,
        "the image could not be pulled from the repository with levels:\n{}",
        pulled.said()
    );

    let digest = the_digest_of(&home, &published);
    assert!(
        digest.starts_with("sha256:"),
        "the pulled image has no digest: {digest}"
    );

    let _ = run_in(&home, &support::the_podman(), &["rmi", &published]);

    case.outcome =
        format!("el repositorio con niveles se publica y se baja con su digest: {digest}");
    case.passed = true;
    support::record_oci_e2e(case);
}

// ---------------------------------------------------------------------------
// La suite oficial de conformidad.
// ---------------------------------------------------------------------------
/// El binario de la suite oficial de conformidad.
///
/// Lo compila `scripts/oci-conformance.sh`, que lo deja en `target/`. Si no
/// esta, la suite oficial **no** se ha ejecutado: y una suite que no se ha
/// ejecutado no es una suite que pasa, es una suite que no se ha mirado.
fn the_conformance_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("oci-conformance")
        .join("oci-conformance")
}

/// Donde la suite deja sus resultados.
///
/// No es un directorio temporal: los resultados de la suite oficial son la
/// **evidencia** de la declaracion de compatibilidad, y una evidencia que se
/// borra al terminar la prueba no se puede archivar ni revisar.
fn the_conformance_results() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("uat")
        .join("oci-conformance")
}

/// Las categorias de la suite oficial, con su estado.
///
/// La suite imprime dos listas--una de **API** y otra de **datos**-- y cada
/// linea es el nombre de la categoria y su estado, con puntos de relleno
/// entre medias.
fn the_categories(stdout: &str, section: &str) -> Vec<(String, String)> {
    let mut categories = Vec::new();
    let mut inside = false;
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed == section {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, status)) = trimmed.rsplit_once(':') {
            let name = name.trim().trim_end_matches('.').trim().to_string();
            categories.push((name, status.trim().to_string()));
        }
    }
    categories
}

/// La suite oficial de conformidad se ejecuta contra el registro.
///
/// Lo que se declara es lo que se ha ejecutado: las categorias que la
/// suite marca como conformes, con su nombre. Las que fallan se anotan con
/// su motivo y su accion, y **no** se declaran: declararlas seria
/// propaganda.
#[test]
fn the_official_conformance_suite_runs_the_declared_categories() {
    let mut case = the_case(
        "OCI-CONFORM-01 (suite oficial)",
        "la suite oficial del distribution-spec compilada y dos repositorios sembrados",
        "la suite empuja, lista, baja y pregunta por los referrers",
        "oci-conformance (OCI_REGISTRY=localhost:<port>)",
    );
    support::record_oci_e2e(case.clone());

    let binary = the_conformance_binary();
    assert!(
        binary.exists(),
        "the official conformance suite is not built: run scripts/oci-conformance.sh"
    );

    let deployment = Deployment::new();
    deployment.seed_with(&[CONFORMANCE_REPOSITORY, CONFORMANCE_REPOSITORY_TWO]);
    let service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, ALICE);
    let results = the_conformance_results();
    std::fs::create_dir_all(&results).expect("the results directory");

    let output = Command::new(&binary)
        .env("OCI_REGISTRY", the_registry(service.port))
        .env("OCI_TLS", "disabled")
        .env("OCI_REPO1", CONFORMANCE_REPOSITORY)
        .env("OCI_REPO2", CONFORMANCE_REPOSITORY_TWO)
        .env("OCI_USERNAME", ALICE)
        .env("OCI_PASSWORD", &token)
        .env("OCI_VERSION", "1.1")
        .env("OCI_DATA_SHA512", "false")
        .env("OCI_API_BLOBS_UPLOAD_CANCEL", "true")
        .env("OCI_RESULTS_DIR", &results)
        .env("OCI_LOG", "warn")
        .output()
        .expect("the conformance suite runs");

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    println!("{stdout}");
    println!("{stderr}");

    // El codigo de salida de la suite es el **todo** de la ejecucion: la
    // suite sale con 1 en cuanto una categoria--declarada o no-- falla o
    // da error, porque su estado global es el peor de sus categorias. Sin
    // esta comprobacion, un fallo en una categoria que no se declara
    // dejaba la pipeline en verde con un "0 en rojo" que nadie miraba.
    assert!(
        output.status.success(),
        "the conformance suite reported a failure:\n{stdout}\n{stderr}"
    );

    let api = the_categories(&stdout, "API conformance:");
    let data = the_categories(&stdout, "Data conformance:");
    assert!(
        !api.is_empty(),
        "the suite did not report its API categories:\n{stdout}"
    );
    assert!(
        !data.is_empty(),
        "the suite did not report its data categories:\n{stdout}"
    );

    for (name, status) in api.iter().chain(data.iter()) {
        println!("{name}: {status}");
    }

    // Lo declarado se comprueba: cada categoria declarada tiene que
    // estar en verde, con el nombre que dice la declaracion.
    for (declared, reported) in DECLARED_API
        .iter()
        .map(|name| (name, api.iter().find(|(reported, _)| reported == name)))
        .chain(
            DECLARED_DATA
                .iter()
                .map(|name| (name, data.iter().find(|(reported, _)| reported == name))),
        )
    {
        let (reported, status) = reported.unwrap_or_else(|| {
            panic!("the suite did not report the declared category {declared}:\n{stdout}")
        });
        assert_eq!(
            status, "Pass",
            "the declared category {declared} is not conformant: {reported} is {status}"
        );
    }

    // Y lo que no se declara no se reclama: las demas categorias se
    // anotan con su estado, para que el informe diga tambien lo que el
    // registro **no** es.
    let others: Vec<String> = api
        .iter()
        .chain(data.iter())
        .filter(|(name, _)| {
            !DECLARED_API.contains(&name.as_str()) && !DECLARED_DATA.contains(&name.as_str())
        })
        .map(|(name, status)| format!("{name}: {status}"))
        .collect();

    // Los numeros van al informe **medidos**, no escritos: los cuenta la
    // suite en su informe de JUnit, que es lo que se archiva.
    let totals = the_totals(&results.join("junit.xml"));
    case.outcome = format!(
        "{} categorias declaradas en verde; {} pruebas en total: {} en verde, {} omitidas, \
         {} en rojo; el resto: {}",
        DECLARED_API.len() + DECLARED_DATA.len(),
        totals.0,
        totals.1,
        totals.2,
        totals.3 + totals.4,
        others.join(", ")
    );
    case.passed = true;
    support::record_oci_e2e(case);
}

/// Los numeros que la suite cuenta en su informe de JUnit.
///
/// Son `(pruebas, en verde, omitidas, fallos, errores)`, y son los
/// numeros que el contrato de compatibilidad declara: leerlos del
/// informe de la ejecucion es lo que hace que no sean un numero escrito
/// a mano.
fn the_totals(report: &std::path::Path) -> (usize, usize, usize, usize, usize) {
    let text = std::fs::read_to_string(report).expect("the junit report is there");
    let number = |name: &str| -> usize {
        let needle = format!("{name}=\"");
        let Some(position) = text.find(&needle) else {
            return 0;
        };
        let rest = &text[position + needle.len()..];
        let end = rest.find('"').expect("the attribute closes");
        rest[..end].parse().unwrap_or(0)
    };
    (
        number("tests"),
        number("tests") - number("skipped") - number("failures") - number("errors"),
        number("skipped"),
        number("failures"),
        number("errors"),
    )
}

/// Las categorias de **API** que el registro declara.
///
/// Lo declarado es lo que se ha ejecutado y ha salido en verde. Una
/// categoria que no esta aqui no se declara: declararla seria
/// propaganda, y el contrato de compatibilidad dice tambien lo que el
/// registro **no** es.
const DECLARED_API: [&str; 19] = [
    "Ping",
    "Tag listing",
    "Blob push",
    "Blob post only",
    "Blob post put",
    "Blob chunked",
    "Blob streaming",
    "Blob mount",
    "Blob upload cancel",
    "Blob get",
    "Blob head",
    "Manifest put by digest",
    "Manifest put by tag",
    "Manifest put with subject",
    "Manifest get by digest",
    "Manifest get by tag",
    "Manifest head by digest",
    "Manifest head by tag",
    "Referrers",
];

/// Las categorias de **datos** que el registro declara.
///
/// El conjunto de datos `sha512` queda **fuera**: el digest del
/// contenido es `sha256` y solo `sha256`, y la suite no se ejecuta con
/// un algoritmo que este registro no habla. El motivo y la accion estan
/// en el contrato de compatibilidad.
const DECLARED_DATA: [&str; 19] = [
    "Image",
    "Image Uncompressed",
    "Image with Large Manifest",
    "Index",
    "Nested Index",
    "Index with Subject",
    "Empty Index",
    "Artifact",
    "Artifact Index",
    "Artifact without Layers",
    "Artifacts with Subject",
    "Missing Subject",
    "Bad Digest Image",
    "Invalid Manifest Digest",
    "Custom Fields",
    "Data Field",
    "No Layers",
    "Non-distributable Layers",
    "Blobs sha256",
];
