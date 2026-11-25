#!/usr/bin/env python3
"""El plan de la puerta del dia a dia: que ha cambiado, que toca y que se corre.

`scripts/test-affected.sh` es la puerta que se abre cada diez minutos. Ese
script decide COMO se corre; este fichero decide QUE. La separacion tiene un
motivo: la decision necesita los METADATOS del workspace --que fichero es de
que paquete y quien depende de quien-- y eso es una pregunta de datos, no de
shell.

Subcomandos:

  plan     lee los ficheros cambiados (por la entrada estandar) y los
           metadatos, y escribe el plan: las causas, los paquetes afectados y
           los gates con su razon.
  explain  imprime el plan para una persona: que cambio, que toca y por que.
  report   junta el plan con el resultado de los gates y escribe el informe
           JSON.

La dedupecion de los paquetes afectados es por DEPENDENCIAS y no por rutas:
cambiar el nucleo toca el nucleo, la API y todo lo de encima. Y un paquete
cuyo codigo no ha cambiado pero cuyo proveedor si puede tener un resultado
distinto, y por eso tambien se prueban.

Códigos de salida:
  0  el plan se ha escrito, o el informe dice PASS
  1  el informe dice FAIL o NOT_RUN
  2  el plan no se se ha podido calcular; el shell lo trata como fallo

`main()` PROPAGA el codigo del subcomando (`sys.exit`): descartarlo hacia que
`report` saliera siempre con 0 aunque el informe dijera FAIL o NOT_RUN.

Contrato del modo `--plan` (HSP-293): un plan no ejecuta nada, asi que todos
sus gates son NOT_RUN por construccion. Eso NO es un fallo: `report --mode plan`
sale con 0 de forma deliberada, y el informe sigue diciendo result NOT_RUN.
Solo el modo `run` convierte FAIL o NOT_RUN en un codigo distinto de 0.

Propiedad de los ficheros (HSP-293):
  - `build.rs` de un paquete es del paquete (`Package.owns`).
  - Un fichero que no es Markdown dentro del directorio de un miembro que no
    es la raiz (por ejemplo un `.sql` o `.json` leido con `include_str!`) es
    del miembro: el codigo puede incluirlo en tiempo de compilacion.
  - Un `.rs` que no es de ningun paquete NO se descarta como `other`: el plan
    falla (codigo 2) con el motivo, porque no se sabe que gate lo prueba. No
    se intenta adivinar el dueno de los ficheros incluidos desde fuera del
    directorio de su paquete (`include_str!("../../x")`): ese caso no es
    viable sin analizar el codigo, y queda cubierto por el fallo explicito
    solo si el fichero es `.rs`.
"""
import argparse
import json
import os
import subprocess
import sys

SCHEMA = "hodei.test-affected/v1"

# El motor de politicas: los paquetes que dependen de el son los que
# evaluan una politica Cedar, y por eso una politica cambiada les toca.
POLICY_ENGINE = "cedar-policy"

# Los directorios donde vive el codigo de un paquete. El `build.rs` vive en
# la raiz del paquete, y con el se creeria que TODO el repositorio es de la
# API.
CODE_DIRECTORIES = ("src", "tests", "benches", "examples")

# Los gates especiales que ya existen en el repositorio.
OPENAPI_GATE = "openapi"
FRONTEND_GATE = "frontend"
CI_SUITES_GATE = "ci_suites"
COMPOSE_GATE = "compose-smoke"


def is_deploy_file(path):
    """Si el fichero cambia lo que `scripts/compose-smoke.sh` verifica (HSP-273).

    Los ficheros compose, la imagen (`Containerfile`), el despliegue de Garage
    y el propio smoke y sus ayudantes.
    """
    name = os.path.basename(path)
    directory = os.path.dirname(path)
    if directory == "" and (
        (name.startswith("compose") and name.endswith((".yaml", ".yml")))
        or name == "compose.env.example"
        or name == "Containerfile"
    ):
        return True
    if path.startswith("deploy/garage/"):
        return True
    return directory == "scripts" and name.startswith("compose-smoke")


def fail(message):
    """Un fallo del plan: el shell lo traduce a un fallo del gate."""
    print(message, file=sys.stderr)
    raise SystemExit(2)


# ───────────────────────── los metadatos ─────────────────────────


class Package:
    """Un paquete del workspace y lo que se sabe de el."""

    def __init__(self, raw):
        self.name = raw["name"]
        self.manifest_path = raw["manifest_path"]
        self.directory = os.path.dirname(raw["manifest_path"])
        self.targets = raw["targets"]
        self.src_paths = [target["src_path"] for target in self.targets]
        self.has_tests = any("test" in target["kind"] for target in self.targets)
        # Los directorios donde vive el codigo de este paquete.
        self.code_directories = set()
        for src_path in self.src_paths:
            directory = os.path.dirname(src_path)
            if os.path.basename(directory) in CODE_DIRECTORIES:
                self.code_directories.add(directory)
        # Los paquetes del workspace de los que depende, por METADATOS: la
        # dependencia con ruta se resuelve contra el manifiesto que la nombra,
        # y no contra un nombre que puede estar renombrado.
        self.depends_on = set()
        # Si habla Cedar.
        self.speaks_policy = False

    def owns(self, path):
        """Si el fichero es de este paquete.

        El path es ABSOLUTO: los metadatos dan las rutas en absoluto y el
        diff las da en relativo, y los dos lados tienen que hablar el mismo
        idioma.
        """
        if path in self.src_paths or path == self.manifest_path:
            return True
        if path == os.path.join(self.directory, "build.rs"):
            return True
        return any(path.startswith(directory + "/") for directory in self.code_directories)

    def owns_asset(self, path):
        """Si el fichero esta dentro del directorio del paquete y no es doc.

        Cubre los ficheros que el codigo incluye con `include_str!` o
        `include_bytes!`. Los Markdown se excluyen: documentar no cambia el
        resultado de las pruebas.
        """
        if path.endswith(".md"):
            return False
        return path.startswith(self.directory + "/")


def load_metadata(root):
    """Los metadatos del workspace: la fuente de verdad del grafo."""
    completed = subprocess.run(
        [
            "cargo",
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
            os.path.join(root, "Cargo.toml"),
        ],
        capture_output=True,
        text=True,
        cwd=root,
    )
    if completed.returncode != 0:
        fail(
            "los metadatos del workspace no se pudieron leer; sin ellos no se puede saber "
            "que paquetes estan afectados:\n" + completed.stderr.strip()
        )
    try:
        return json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        fail(f"los metadatos del workspace no son JSON legible: {error}")


def build_packages(metadata):
    """Los paquetes del workspace y el grafo de dependencias entre ellos."""
    packages = [Package(raw) for raw in metadata["packages"]]
    by_manifest = {package.manifest_path: package for package in packages}

    for raw, package in zip(metadata["packages"], packages):
        for dependency in raw["dependencies"]:
            if dependency["name"] == POLICY_ENGINE:
                package.speaks_policy = True
            path = dependency.get("path")
            if not path:
                # Una dependencia del registro no es un paquete de este
                # workspace: no puede arrastrar a nadie de aqui.
                continue
            dependency_package = by_manifest.get(os.path.join(path, "Cargo.toml"))
            if dependency_package is not None and dependency_package.name != package.name:
                package.depends_on.add(dependency_package.name)

    return packages


def dependents_of(packages):
    """Quien depende de cada paquete, segun el grafo de los metadatos."""
    dependents = {package.name: set() for package in packages}
    for package in packages:
        for dependency in package.depends_on:
            if dependency in dependents:
                dependents[dependency].add(package.name)
    return dependents


# ───────────────────────── las causas ─────────────────────────


def detect_causes(files, packages, root):
    """Que ha cambiado, clasificado por causa.

    Una causa es la razon por la que se elige un gate. Sin causa no hay gate,
    y un cambio que no se clasifica se declara explicitamente en vez de
    ignorarse en silencio.
    """
    causes = []

    def add(name, matched, reason):
        if matched:
            causes.append({"cause": name, "files": sorted(matched), "reason": reason})

    owned = {}
    for path in files:
        absolute = os.path.normpath(os.path.join(root, path))
        for package in packages:
            if package.owns(absolute):
                owned.setdefault(package.name, []).append(path)
                break
        else:
            # Ficheros incluidos en tiempo de compilacion: el miembro mas
            # especifico (directorio mas largo) que no es la raiz.
            candidates = [
                package
                for package in packages
                if os.path.normpath(package.directory) != os.path.normpath(root)
                and package.owns_asset(absolute)
            ]
            if candidates:
                best = max(candidates, key=lambda package: len(package.directory))
                owned.setdefault(best.name, []).append(path)

    add(
        "lockfile",
        [path for path in files if path == "Cargo.lock"],
        "Cargo.lock cambio: las dependencias de cualquier paquete pueden haber cambiado",
    )
    add(
        "workspace-manifest",
        [path for path in files if path == "Cargo.toml"],
        "el manifiesto del workspace cambio: los miembros y sus dependencias pueden haber cambiado",
    )
    add(
        "code",
        sorted(path for paths in owned.values() for path in paths),
        "el codigo de un paquete del workspace cambio",
    )
    add(
        "policy",
        [path for path in files if path.endswith(".cedar")],
        "una politica Cedar cambio: los paquetes que la evaluan tienen que volver a probarse",
    )
    add(
        "config",
        [
            path
            for path in files
            if path.startswith("config/") and not path.endswith(".cedar")
        ],
        "la configuracion cambio: la API la carga al arrancar",
    )
    add(
        "openapi",
        [path for path in files if path.startswith("docs/openapi/")],
        "el contrato de la API cambio: la API lo sirve y el gate comprueba que valida",
    )
    add(
        "frontend",
        [path for path in files if path.startswith("frontend/")],
        "el frontend cambio: sus componentes tienen que seguir pasando",
    )
    add(
        "deploy",
        [path for path in files if is_deploy_file(path)],
        "el despliegue cambio (compose, Containerfile o Garage): el smoke con un runtime real "
        "es lo unico que lo verifica",
    )
    add(
        "pipeline",
        [path for path in files if path == "pipeline.kts"],
        "la pipeline cambio: la segmentacion de las suites cambio y su guardian la comprueba",
    )

    classified = set()
    for cause in causes:
        classified.update(cause["files"])
    unclassified = [path for path in files if path not in classified]
    orphan_sources = sorted(path for path in unclassified if path.endswith(".rs"))
    if orphan_sources:
        fail(
            "hay ficheros .rs que no son de ningun paquete del workspace y no se sabe que gate "
            "los prueba: " + ", ".join(orphan_sources)
        )
    add(
        "other",
        unclassified,
        "ficheros que no son codigo de ningun gate: se declaran, pero no hay nada que correr",
    )

    return causes, owned


# ───────────────────────── el plan ─────────────────────────


def build_plan(args, files):
    root = os.path.abspath(args.root)
    metadata = load_metadata(root)
    packages = build_packages(metadata)
    by_name = {package.name: package for package in packages}
    dependents = dependents_of(packages)

    # El paquete de la raiz: el que compone el servicio y el que carga la
    # configuracion y el contrato de la API.
    root_package = None
    for package in packages:
        if os.path.normpath(package.manifest_path) == os.path.normpath(
            os.path.join(root, "Cargo.toml")
        ):
            root_package = package
            break

    causes, owned = detect_causes(files, packages, root)
    cause_names = {cause["cause"] for cause in causes}

    # El alcance: que paquetes entran, y por que. Un paquete puede entrar
    # por varias causas a la vez, y cada una se guarda: la razon que se
    # explica es la mas especifica, pero ninguna se pierde.
    #   direct    su codigo cambio
    #   dependent su proveedor cambio
    #   global    el grafo de dependencias cambio
    #   api       la API carga o sirve lo que cambio
    #   policy    el paquete evalua politicas Cedar
    scope = {}

    def entry_for(package_name):
        if package_name is None:
            return None
        return scope.setdefault(
            package_name, {"direct": False, "graph": False, "reasons": []}
        )

    def add_reason(package_name, kind, text):
        entry = entry_for(package_name)
        if entry is not None:
            entry["reasons"].append({"kind": kind, "text": text})

    # El codigo de un paquete: el paquete y todos los que dependen de el.
    # El cierre se recorre entero, y no solo por los paquetes que aun no
    # estaban: un paquete que cambio Y tiene dependientes tiene que
    # arrastrar a sus dependientes tambien.
    for package_name, paths in sorted(owned.items()):
        entry = entry_for(package_name)
        entry["direct"] = True
        add_reason(package_name, "direct", ", ".join(sorted(paths)))

    # Cada paquete arrastrado recuerda el paquete cuyo codigo cambio de
    # verdad: el cierre puede pasar por otros, y la razon tiene que senalar
    # al origen y no al intermediario.
    origin = {}
    frontier = [(name, name) for name in sorted(owned)]
    while frontier:
        current, root = frontier.pop(0)
        for dependent in sorted(dependents.get(current, ())):
            if dependent in origin or dependent in owned:
                continue
            origin[dependent] = (current, root)
            frontier.append((dependent, root))
    for dependent, (_, root) in sorted(origin.items()):
        add_reason(dependent, "dependent", f"depende de {root}, cuyo codigo cambio")

    # El grafo de dependencias: cualquier paquete puede verse afectado.
    if "lockfile" in cause_names or "workspace-manifest" in cause_names:
        reason = (
            "Cargo.lock cambio"
            if "lockfile" in cause_names
            else "el manifiesto del workspace cambio"
        )
        for package in packages:
            entry = entry_for(package.name)
            entry["graph"] = True
            add_reason(package.name, "global", reason)

    # La configuracion y el contrato los carga y los sirve la API.
    if "config" in cause_names:
        add_reason(
            root_package.name if root_package else None,
            "api",
            "la configuracion cambio",
        )
    if "openapi" in cause_names:
        add_reason(
            root_package.name if root_package else None,
            "api",
            "el contrato de la API cambio",
        )

    # Las politicas Cedar las evaluan los paquetes que hablan Cedar.
    if "policy" in cause_names:
        for package in packages:
            if package.speaks_policy:
                add_reason(package.name, "policy", "una politica Cedar cambio")
        add_reason(
            root_package.name if root_package else None,
            "api",
            "una politica Cedar cambio",
        )

    # ── los gates ──

    gates = []

    def add_gate(gate, kind, package, command, reason):
        gates.append(
            {
                "gate": gate,
                "kind": kind,
                "package": package,
                "command": command,
                "reason": reason,
            }
        )

    for package_name in sorted(scope):
        package = by_name.get(package_name)
        if package is None:
            continue
        entry = scope[package_name]
        reasons = entry["reasons"]

        def text_of(kind):
            for reason in reasons:
                if reason["kind"] == kind:
                    return reason["text"]
            return ""

        # La razon del gate de pruebas: la mas especifica de las que hay.
        specific = next(
            (reason for reason in reasons if reason["kind"] in ("direct", "dependent", "api", "policy")),
            reasons[0] if reasons else None,
        )
        test_reason = (
            f"el codigo de {package_name} cambio ({specific['text']})"
            if specific and specific["kind"] == "direct"
            else specific["text"] if specific
            else "el paquete esta en el alcance"
        )

        if entry["direct"]:
            if args.fmt:
                add_gate(
                    f"fmt:{package_name}",
                    "fmt",
                    package_name,
                    f"cargo fmt -p {package_name} -- --check",
                    f"el codigo de {package_name} cambio ({text_of('direct')}): el formato tiene que "
                    "ser el del repositorio",
                )
            if args.clippy:
                add_gate(
                    f"clippy:{package_name}",
                    "clippy",
                    package_name,
                    f"cargo clippy -p {package_name} --all-targets -- -D warnings",
                    f"el codigo de {package_name} cambio ({text_of('direct')}): los avisos se "
                    "comprueban sobre el codigo nuevo",
                )
        elif entry["graph"]:
            if args.clippy:
                add_gate(
                    f"clippy:{package_name}",
                    "clippy",
                    package_name,
                    f"cargo clippy -p {package_name} --all-targets -- -D warnings",
                    f"{text_of('global')}: las versiones nuevas pueden traer avisos nuevos",
                )

        if args.tests and (reasons or entry["graph"]):
            add_gate(
                f"test:{package_name}",
                "test",
                package_name,
                f"cargo test -p {package_name}",
                f"{test_reason}: sus pruebas pueden cambiar de resultado",
            )

    # Los gates especiales: los que ya existen en el repositorio.
    if "openapi" in cause_names or args.force_openapi:
        add_gate(
            OPENAPI_GATE,
            "openapi",
            None,
            "bash scripts/check-openapi.sh",
            "el contrato de la API cambio: el gate comprueba que valida y que el bundle "
            "versionado esta en sincronia",
        )
    if "frontend" in cause_names:
        add_gate(
            FRONTEND_GATE,
            "frontend",
            None,
            "cd frontend && npm test",
            "el frontend cambio: sus componentes tienen que seguir pasando",
        )
    if "deploy" in cause_names:
        add_gate(
            COMPOSE_GATE,
            "compose",
            None,
            "bash scripts/compose-smoke.sh",
            "el despliegue cambio: el smoke arranca los perfiles compose con podman y comprueba "
            "que el perfil externo falla cerrado (exit 77 = NOT_RUN, nunca verde)",
        )
    if "pipeline" in cause_names:
        if root_package is not None and any(
            target["name"] == CI_SUITES_GATE and "test" in target["kind"]
            for target in root_package.targets
        ):
            add_gate(
                CI_SUITES_GATE,
                "ci_suites",
                None,
                f"cargo test -p {root_package.name} --test {CI_SUITES_GATE}",
                "la pipeline cambio: la segmentacion de las suites cambio y su guardian la "
                "comprueba en los dos sentidos",
            )

    affected = []
    for package_name in sorted(scope):
        package = by_name.get(package_name)
        if package is None:
            continue
        entry = scope[package_name]
        reasons = entry["reasons"]
        specific = next(
            (reason for reason in reasons if reason["kind"] in ("direct", "dependent", "api", "policy")),
            reasons[0] if reasons else None,
        )
        affected.append(
            {
                "name": package_name,
                "scope": specific["kind"] if specific else "global",
                "reason": specific["text"] if specific else "",
                "has_test_targets": package.has_tests,
            }
        )

    return {
        "schema": SCHEMA,
        "root": root,
        "repository": {
            "root": root,
            "commit": args.commit,
            "branch": args.branch,
            "base": {
                "requested": args.base_requested,
                "commit": args.base_commit,
                "merge_base": args.merge_base,
            },
        },
        "causes": causes,
        "affected_crates": affected,
        "gates": gates,
    }


def cmd_plan(args):
    files = sorted({line.strip() for line in sys.stdin.read().splitlines() if line.strip()})
    plan = build_plan(args, files)

    with open(args.out, "w", encoding="utf-8") as handle:
        json.dump(plan, handle, indent=2, ensure_ascii=False)
        handle.write("\n")

    with open(args.tsv, "w", encoding="utf-8") as handle:
        for gate in plan["gates"]:
            handle.write(
                "\t".join(
                    [
                        gate["gate"],
                        gate["kind"],
                        # `-` and not an empty field: the shell reads the TSV with
                        # `IFS=$'\t' read`, and tab is whitespace there, so an empty
                        # package would collapse and shift command/reason (HSP-273).
                        gate["package"] or "-",
                        gate["command"],
                        gate["reason"],
                    ]
                )
                + "\n"
            )


def cmd_explain(args):
    with open(args.plan, encoding="utf-8") as handle:
        plan = json.load(handle)

    print("── causas ──")
    if not plan["causes"]:
        print("  (ninguna: no hay ficheros cambiados)")
    for cause in plan["causes"]:
        print(f"  {cause['cause']:<20} {cause['reason']}")
        for path in cause["files"]:
            print(f"  {'':<20}   {path}")

    print("── paquetes afectados ──")
    if not plan["affected_crates"]:
        print("  (ninguno)")
    for crate in plan["affected_crates"]:
        print(f"  {crate['name']:<28} {crate['scope']:<11} {crate['reason']}")

    print("── gates ──")
    if not plan["gates"]:
        print("  (ninguno: no hay nada que correr)")
    for gate in plan["gates"]:
        print(f"  {gate['gate']:<28} {gate['command']}")
        print(f"  {'':<28} porque {gate['reason']}")


def cmd_report(args):
    with open(args.plan, encoding="utf-8") as handle:
        plan = json.load(handle)

    results = {}
    if args.results and os.path.exists(args.results):
        with open(args.results, encoding="utf-8") as handle:
            for line in handle:
                line = line.rstrip("\n")
                if not line:
                    continue
                parts = line.split("\t")
                if len(parts) < 4:
                    continue
                gate, status, duration, exit_code = parts[0], parts[1], parts[2], parts[3]
                note = parts[4] if len(parts) > 4 else ""
                results[gate] = {
                    "status": status,
                    "duration_ms": int(duration or 0),
                    "exit_code": int(exit_code or 0),
                    "note": note,
                }

    gates = []
    total_duration = 0
    for gate in plan["gates"]:
        result = results.get(gate["gate"])
        if result is None:
            status = "NOT_RUN"
            duration = 0
            exit_code = None
            note = "el gate no se ha ejecutado"
        else:
            status = result["status"]
            duration = result["duration_ms"]
            exit_code = result["exit_code"]
            note = result["note"]
        total_duration += duration
        entry = dict(gate)
        entry["status"] = status
        entry["duration_ms"] = duration
        entry["exit_code"] = exit_code
        if note:
            entry["note"] = note
        gates.append(entry)

    if any(gate["status"] == "FAIL" for gate in gates):
        result = "FAIL"
    elif any(gate["status"] == "NOT_RUN" for gate in gates):
        # Un gate que no se ha ejecutado NO es un gate que ha pasado: un
        # NOT_RUN nunca cuenta como PASS.
        result = "NOT_RUN"
    elif gates:
        result = "PASS"
    else:
        result = "NOT_RUN"

    report = {
        "schema": SCHEMA,
        "mode": args.mode,
        "result": result,
        "duration_ms": total_duration,
        "limits": args.limits,
        "repository": plan["repository"],
        "causes": plan["causes"],
        "affected_crates": plan["affected_crates"],
        "gates": gates,
    }
    with open(args.out, "w", encoding="utf-8") as handle:
        json.dump(report, handle, indent=2, ensure_ascii=False)
        handle.write("\n")

    print(f"── informe ──")
    print(f"  modo: {args.mode}")
    print(f"  resultado: {result}")
    print(f"  gates: {len(gates)} ({sum(1 for g in gates if g['status'] == 'PASS')} pass, "
          f"{sum(1 for g in gates if g['status'] == 'FAIL')} fail, "
          f"{sum(1 for g in gates if g['status'] == 'NOT_RUN')} not_run)")
    print(f"  informe: {args.out}")

    if args.mode == "plan":
        # Un plan no ejecuta nada: sus NOT_RUN son por construccion y no un
        # fallo. Contrato deliberado: el modo plan sale con 0.
        return 0
    if result == "PASS":
        return 0
    if result == "FAIL":
        print("  hay gates que han fallado: lo afectado NO esta en verde", file=sys.stderr)
        return 1
    if not gates:
        # No habia nada que correr: no es un resultado, y no se ha afirma
        # ninguna vez que lo este.
        print("  no habia gates que correr: no se ha ejecutado ninguna prueba", file=sys.stderr)
        return 0
    print("  hay gates que NO se han ejecutado: un NOT_RUN no es un PASS", file=sys.stderr)
    return 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    plan = subparsers.add_parser("plan", help="calcula el plan de lo afectado")
    plan.add_argument("--root", required=True)
    plan.add_argument("--base-requested", default="main")
    plan.add_argument("--base-commit", default="")
    plan.add_argument("--merge-base", default="")
    plan.add_argument("--commit", default="")
    plan.add_argument("--branch", default="")
    plan.add_argument("--fmt", type=int, default=1)
    plan.add_argument("--clippy", type=int, default=1)
    plan.add_argument("--tests", type=int, default=1)
    plan.add_argument("--force-openapi", type=int, default=0)
    plan.add_argument("--out", required=True)
    plan.add_argument("--tsv", required=True)
    plan.set_defaults(handler=cmd_plan)

    explain = subparsers.add_parser("explain", help="imprime el plan")
    explain.add_argument("--plan", required=True)
    explain.set_defaults(handler=cmd_explain)

    report = subparsers.add_parser("report", help="escribe el informe JSON")
    report.add_argument("--plan", required=True)
    report.add_argument("--results", default="")
    report.add_argument("--out", required=True)
    report.add_argument("--mode", default="run")
    report.add_argument("--limits", default="none")
    report.set_defaults(handler=cmd_report)

    args = parser.parse_args()
    code = args.handler(args)
    sys.exit(code or 0)


if __name__ == "__main__":
    main()
