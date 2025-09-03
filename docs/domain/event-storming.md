# **Sesión de Event Storming: Ecosistema Hodei**

**Objetivo**: Mapear los flujos de negocio y descubrir las interacciones entre los contextos para validar y refinar nuestro modelo de dominio.

**Leyenda de "Notas Adhesivas"**:

* 🟠 **Evento de Dominio**: Un hecho de negocio que ocurrió en el pasado. (El corazón de la sesión).
* 🔵 **Comando**: La intención de un usuario o sistema de ejecutar una acción.
* 👤 **Actor**: El usuario, rol o sistema que inicia un comando.
* 🟡 **Agregado**: La entidad de dominio que procesa un comando y emite un evento.
* 🟣 **Política**: Una regla de negocio que reacciona a un evento para disparar un nuevo comando. (Siempre que..., Entonces...).
* 🟢 **Read Model / Vista**: Una proyección de datos optimizada para consultas de la UI.
* 🟥 **Sistema Externo**: Un servicio fuera de nuestro control directo (ej. un proveedor de email, un escáner).

-----

### Flujo 1: Ciclo de Vida de la Organización y Membresía

**Actores**: 👤 `SuperAdmin`, 👤 `Admin de Organización`, 👤 `Nuevo Usuario`

**(Inicio del Flujo)**

1.  👤 `SuperAdmin` -\> 🔵 `CrearOrganización` (nombre: "AcmeCorp", owner\_email: "admin@acme.com") -\> 🟡 `Organization`

    * **Decisión**: ¿El nombre "AcmeCorp" ya existe? ¿El owner es válido?
    * 🟠 **`OrganizaciónCreada`** (hrn: "...", nombre: "AcmeCorp", owner\_hrn: "...")

2.  🟢 **Vista `ListaDeOrganizaciones` actualizada.**

3.  👤 `Admin de Organización` -\> 🔵 `InvitarMiembro` (email: "dev@acme.com", rol: "Member") -\> 🟡 `Invitation`

    * **Decisión**: ¿El usuario ya es miembro? ¿La invitación es válida?
    * 🟠 **`MiembroInvitado`** (org\_hrn: "...", email: "dev@acme.com", inviter\_hrn: "...")

4.  🟥 **Sistema de Email** -\> 📧 `EmailDeInvitaciónEnviado`

5.  👤 `Nuevo Usuario` (hace clic en el enlace del email) -\> 🟥 **Keycloak (IdP)** -\> 🔵 `AceptarInvitación` (token: "xyz...") -\> 🟡 `Invitation` + `Member`

    * **Decisión**: ¿El token de invitación es válido y no ha expirado? ¿El usuario ya está registrado en Hodei?
    * *(Si no está registrado)* 🟠 **`UsuarioRegistrado`** (hrn: "...", email: "dev@acme.com")
    * 🟠 **`MiembroUnidoAOrganización`** (member\_hrn: "...", user\_hrn: "...", org\_hrn: "...")
    * 🟠 **`InvitaciónAceptada`** (token: "xyz...")

6.  🟢 **Vista `MiembrosDeOrganización` actualizada.**

7.  🟢 **Vista `PerfilDeUsuario` actualizada con la nueva membresía.**

-----

### Flujo 2: Ciclo de Vida del Repositorio y Políticas

**Actores**: 👤 `Admin de Organización`, ⏰ `Disparador Programado (Scheduler)`

1.  👤 `Admin de Organización` -\> 🔵 `CrearRepositorio` (nombre: "npm-proxy", tipo: Proxy, formato: Npm, remoteUrl: "[https://registry.npmjs.org](https://www.google.com/search?q=https://registry.npmjs.org)") -\> 🟡 `Repository`

    * **Decisión**: ¿El nombre es único en la organización? ¿El tipo y formato son compatibles? ¿El usuario tiene permisos?
    * 🟠 **`RepositorioCreado`** (hrn: "...", tipo: Proxy, formato: Npm)

2.  🟢 **Vista `DetalleDeRepositorio` creada.**

3.  👤 `Admin de Organización` -\> 🔵 `CrearPolíticaDeRetención` (nombre: "Limpiar Snapshots", reglas: [{regex: ".\*-SNAPSHOT", max\_edad: 30 días, acción: Delete}]) -\> 🟡 `RetentionPolicy`

    * **Decisión**: ¿Las reglas son válidas?
    * 🟠 **`PolíticaDeRetenciónCreada`** (hrn: "...", repo\_hrn: "...")

4.  ⏰ `Disparador Programado` (ej. cada noche a las 2 AM) -\> 🔵 `AplicarPolíticasDeRetención` (repo\_hrn: "...") -\> 🟡 `Repository`

    * **Decisión**: Iterar sobre todos los `PackageVersion` del repositorio y aplicar las reglas de la política.
    * *(Si se borran artefactos)* 🟠 **`VersionesDePaqueteDepuradasPorPolítica`** (repo\_hrn: "...", depurados\_hrns: ["...", "..."], policy\_hrn: "...")
    * 🟠 **`AplicaciónDePolíticaDeRetenciónFinalizada`** (policy\_hrn: "...", repo\_hrn: "...", resultado: {borrados: 2, archivados: 0})

-----

### Flujo 3: Ingesta de un Artefacto (El Flujo Central)

**Actores**: 👤 `Sistema CI/CD`

1.  👤 `Sistema CI/CD` -\> 🔵 `PublicarVersiónDePaquete` (coordenadas, ficheros: [jar, pom, sources]) -\> 🟡 `PackageVersion`

    * **Decisión Clave**: ¿El principal tiene permisos de escritura en el repositorio? ¿El repositorio permite este tipo de despliegue (ej. snapshots)? ¿La cuota de almacenamiento no se excede?
    * *(Para cada fichero)* 🟠 **`ArtefactoFísicoAlmacenado`** (hrn: "...", content\_hash: "sha256:abc...")
    * 🟠 **`VersiónDePaquetePublicada`** (hrn: "...", repo\_hrn: "...", coordenadas: {...}, publisher\_hrn: "...")

2.  🟢 **Vista `DetalleDeRepositorio` actualizada con el nuevo paquete.**

3.  🟣 **Política del Sistema**: **Siempre que** `VersiónDePaquetePublicada`...

    * **Entonces** -\> 🔵 `SolicitarEscaneoDeSeguridad` (package\_version\_hrn: "...")
    * **Entonces** -\> 🔵 `SolicitarGeneraciónDeSBOM` (package\_version\_hrn: "...")
    * **Entonces** -\> 🔵 `SolicitarGeneraciónDeProcedenciaSLSA` (package\_version\_hrn: "...")
    * **Entonces** -\> 🔵 `IndexarVersiónDePaquete` (package\_version\_hrn: "...")

-----

### Flujo 4: Reacción de Seguridad y Cumplimiento

**Actores**: Este flujo es completamente automatizado, iniciado por una política.

1.  *(Continuación del Flujo 3)* -\> 🔵 `SolicitarEscaneoDeSeguridad` -\> 🟡 `SecurityScanResult`

    * **Decisión**: ¿Qué escáner usar para este tipo de artefacto?
    * 🟠 **`EscaneoDeSeguridadIniciado`** (scan\_hrn: "...", package\_version\_hrn: "...", scanner: "Trivy")

2.  🟥 **Escáner de Vulnerabilidades Externo** -\> ...procesando... -\> 🔵 `CompletarEscaneoDeSeguridad` (scan\_hrn: "...", hallazgos: [...]) -\> 🟡 `SecurityScanResult` + `VulnerabilityOccurrence`

    * **Decisión**: Normalizar los hallazgos contra la base de datos de `VulnerabilityDefinition`.
    * *(Si hay nuevas CVEs)* 🟠 **`DefiniciónDeVulnerabilidadAñadida`** (hrn: "...", cve\_id: "...")
    * 🟠 **`OcurrenciaDeVulnerabilidadRegistrada`** (hrn: "...", package\_version\_hrn: "...", vuln\_def\_hrn: "...")
    * 🟠 **`EscaneoDeSeguridadCompletado`** (hrn: "...", resumen: {critical: 1, high: 5})

3.  🟢 **Vista `PanelDeSeguridad` actualizada.**

4.  🟢 **Vista `DetalleDeArtefacto` actualizada con las vulnerabilidades.**

5.  🟣 **Política de Seguridad**: **Siempre que** `EscaneoDeSeguridadCompletado` con `summary.critical_count > 0`...

    * **Entonces** -\> 🔵 `PonerEnCuarentenaVersiónDePaquete` (hrn: "...", razón: "Vulnerabilidad crítica encontrada")

6.  *(El comando llega al contexto `artifact`)* -\> 🟡 `PackageVersion`

    * **Decisión**: ¿El artefacto ya está en cuarentena o baneado?
    * 🟠 **`EstadoDeVersiónDePaqueteCambiado`** (hrn: "...", estado\_anterior: Active, estado\_nuevo: Quarantined)

7.  🟢 **Vista `DetalleDeArtefacto` muestra ahora una alerta de CUARENTENA.**

-----

### Flujo 5: Reacción de la Cadena de Suministro

**Actores**: Flujo automatizado.

1.  *(Continuación del Flujo 3)* -\> 🔵 `SolicitarGeneraciónDeSBOM` -\> 🟡 `Attestation`

    * **Decisión**: ¿Qué herramienta usar para generar el SBOM?
    * 🟠 **`GeneraciónDeAtestaciónIniciada`** (subject\_hrn: "...", tipo: SBOM\_CycloneDx)

2.  🟥 **Herramienta de SBOM Externa** -\> ...procesando... -\> 🔵 `AlmacenarAtestación` (subject\_hrn: "...", predicado: {...}) -\> 🟡 `Attestation`

    * 🟠 **`AtestaciónGenerada`** (hrn: "...", subject\_hrn: "...", tipo: SBOM\_CycloneDx)

3.  🟣 **Política de Integridad**: **Siempre que** `AtestaciónGenerada`...

    * **Entonces** -\> 🔵 `FirmarAtestación` (hrn: "...", key\_hrn: "...")

4.  🟥 **Servicio de Firma (KMS/Sigstore)** -\> 🔵 `AdjuntarFirmaAAtestación` (hrn: "...", firma: "...") -\> 🟡 `Attestation`

    * 🟠 **`AtestaciónFirmada`** (hrn: "...", key\_hrn: "...")

5.  🟢 **Vista `DetalleDeArtefacto` ahora muestra un enlace al SBOM y la firma.**

-----

### Flujo 6: Consumo de un Artefacto

**Actores**: 👤 `Desarrollador`, 👤 `Sistema CI/CD`

1.  👤 `Desarrollador` -\> 🔵 `DescargarVersiónDePaquete` (hrn: "...") -\> 🟡 `PackageVersion`

    * **Decisión Clave (Autorización)**: ¿El principal tiene permisos de lectura? ¿El artefacto está en estado `Active`? (¡No se pueden descargar artefactos en cuarentena\!)
    * 🟠 **`VersiónDePaqueteDescargada`** (hrn: "...", downloader\_hrn: "...")

2.  🟢 **Vista `DetalleDeArtefacto` incrementa el contador de descargas.**

3.  *(El contexto `analytics` consume el evento para actualizar métricas de uso)*.¡Por supuesto\! La primera sesión de Event Storming sentó las bases. Ahora, extendámosla para explorar flujos de trabajo más complejos y realistas que demuestren la verdadera potencia y robustez del modelo de dominio que hemos diseñado.

En esta sesión ampliada, nos sumergiremos en las características avanzadas que diferencian a un repositorio de artefactos básico de una plataforma de nivel empresarial como Artifactory o Nexus.

-----


### Flujo 7: Interacción con un Repositorio Proxy (Cache Miss & Hit)

**Contexto**: Un desarrollador necesita una dependencia (`lodash@4.17.21`) de un repositorio configurado como proxy de `registry.npmjs.org`.

**Actores**: 👤 `Desarrollador`

#### Escenario A: Cache Miss (La primera vez que se solicita el paquete)

1.  👤 `Desarrollador` (via `npm install`) -\> 🔵 `SolicitarPaqueteDesdeProxy` (coordenadas: "lodash@4.17.21") -\> 🟡 `Repository` (el proxy)

    * **Decisión**: ¿El artefacto está en el caché local? **No**. ¿Ha expirado el TTL de "no encontrado"? **Sí**.
    * 🟠 **`CacheDeProxyNoEncontrado`** (repo\_hrn: "...", coordenadas: "...")

2.  🟣 **Política del Proxy**: **Siempre que** `CacheDeProxyNoEncontrado`...

    * **Entonces** -\> 🔵 `DescargarPaqueteDeRemoto` (remote\_url: "[https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz](https://www.google.com/search?q=https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz)")

3.  🟥 **Repositorio Remoto (NPM Registry)** -\> Devuelve el fichero del paquete.

4.  *(El sistema Hodei ahora actúa como si fuera un cliente subiendo un nuevo artefacto a sí mismo)* -\> 🔵 `PublicarVersiónDePaquete` (en el repo proxy, con los ficheros de `lodash`) -\> 🟡 `PackageVersion`

    * 🟠 **`VersiónDePaqueteCacheadaEnProxy`** (hrn: "...", repo\_hrn: "...", coordenadas: "...")

5.  *(A partir de aquí, se desencadena el flujo estándar de ingesta)*

    * 🟣 **Política del Sistema**: **Siempre que** `VersiónDePaqueteCacheadaEnProxy`...
        * **Entonces** -\> 🔵 `SolicitarEscaneoDeSeguridad`
        * **Entonces** -\> 🔵 `SolicitarGeneraciónDeSBOM`
    * ... (se escanea y se analiza el paquete `lodash` recién cacheado).

6.  **(Finalmente)** El artefacto se sirve al 👤 `Desarrollador`.

#### Escenario B: Cache Hit (Solicitudes subsecuentes)

1.  👤 `Desarrollador` -\> 🔵 `SolicitarPaqueteDesdeProxy` (coordenadas: "lodash@4.17.21") -\> 🟡 `Repository` (el proxy)

    * **Decisión**: ¿El artefacto está en el caché local? **Sí**. ¿El TTL del caché es válido? **Sí**.
    * 🟠 **`VersiónDePaqueteServidaDesdeCache`** (hrn: "...", repo\_hrn: "...")

2.  El artefacto se sirve inmediatamente desde el almacenamiento local de Hodei al 👤 `Desarrollador`.

-----

### Flujo 8: Resolución a Través de un Repositorio Virtual

**Contexto**: Un desarrollador solicita una dependencia (`common-utils@1.2.0`) de un repositorio "virtual" llamado `npm-all`, que agrega `npm-internal` (hosted) y `npm-proxy` (proxy) en ese orden.

**Actores**: 👤 `Desarrollador`

1.  👤 `Desarrollador` -\> 🔵 `SolicitarPaqueteDesdeVirtual` (repo\_hrn: "...npm-all", coordenadas: "common-utils@1.2.0") -\> 🟡 `Repository` (el virtual)

    * **Decisión (Lógica de Resolución)**: ¿Cuál es el orden de resolución? Primero `npm-internal`, luego `npm-proxy`.
    * 🟠 **`ResoluciónVirtualIniciada`** (virtual\_repo\_hrn: "...", coordenadas: "...")

2.  *(El repo virtual delega la búsqueda al primer repo de la lista)* -\> 🔵 `SolicitarPaqueteDesdeRepositorio` (repo\_hrn: "...npm-internal", coordenadas: "...") -\> 🟡 `Repository` (el hosted)

    * **Decisión**: ¿Existe el paquete en `npm-internal`? **Sí**.
    * 🟠 **`VersiónDePaqueteEncontradaEnAgregado`** (virtual\_repo\_hrn: "...", found\_in\_repo\_hrn: "...npm-internal")

3.  El artefacto de `npm-internal` se sirve al 👤 `Desarrollador`.

    * *(Si no se hubiera encontrado, el proceso se repetiría con el siguiente repositorio en la lista, `npm-proxy`, desencadenando el Flujo 7 si fuera necesario)*.
    * 🟠 **`VersiónDePaqueteResueltaViaVirtual`** (hrn\_del\_paquete: "...", servido\_a: "...")

-----

### Flujo 9: Re-evaluación de Seguridad Proactiva (Nueva Vulnerabilidad Descubierta)

**Contexto**: El equipo de seguridad se entera de una nueva vulnerabilidad "zero-day" en una librería popular (ej. `log4j`). Quieren encontrar todos los artefactos afectados que ya existen en el sistema.

**Actores**: 👤 `Equipo de Seguridad` / `Sistema Automatizado`

1.  👤 `Equipo de Seguridad` -\> 🔵 `AñadirDefiniciónDeVulnerabilidad` (CVE-ID: "CVE-2025-XXXX", paquete\_afectado: "log4j-core", rango\_versiones: "\< 2.17.2") -\> 🟡 `VulnerabilityDefinition`

    * **Decisión**: ¿Ya existe esta definición?
    * 🟠 **`DefiniciónDeVulnerabilidadAñadida`** (hrn: "...", source\_id: "CVE-2025-XXXX")

2.  🟣 **Política de Seguridad Proactiva**: **Siempre que** `DefiniciónDeVulnerabilidadAñadida`...

    * **Entonces** -\> 🔵 `DispararRetroalimentaciónDeVulnerabilidad` (vuln\_def\_hrn: "...")

3.  *(El sistema ahora realiza una consulta compleja)* -\> 🟢 **Vista `ComponentesDeSBOMs`**

    * **Consulta**: "Encontrar todos los `PackageVersion` que tienen un componente SBOM `log4j-core` con versión `< 2.17.2`".
    * 🟠 **`PotencialesArtefactosAfectadosIdentificados`** (package\_version\_hrns: ["...", "..."])

4.  *(Para cada artefacto afectado encontrado)* -\> 🔵 `RegistrarOcurrenciaDeVulnerabilidad` (package\_version\_hrn: "...", vuln\_def\_hrn: "...") -\> 🟡 `VulnerabilityOccurrence`

    * 🟠 **`OcurrenciaDeVulnerabilidadRegistradaRetroactivamente`** (hrn: "...", package\_version\_hrn: "...")

5.  *(Esto, a su vez, puede disparar la política de cuarentena del Flujo 4)* -\> 🔵 `PonerEnCuarentenaVersiónDePaquete` -\> 🟠 `EstadoDeVersiónDePaqueteCambiado`

-----

### Flujo 10: Gestión Avanzada de Acceso (Grupos y Políticas)

**Contexto**: Un administrador quiere dar permisos de solo lectura a un equipo de QA sobre un repositorio de "staging".

**Actores**: 👤 `Admin de Organización`

1.  👤 `Admin` -\> 🔵 `CrearGrupo` (nombre: "qa-team", org\_hrn: "...") -\> 🟡 `Group`

    * 🟠 **`GrupoCreado`** (hrn: "...", nombre: "qa-team")

2.  👤 `Admin` -\> 🔵 `AñadirUsuarioAGrupo` (user\_hrn: "...dev3", group\_hrn: "...qa-team") -\> 🟡 `User`

    * 🟠 **`UsuarioAñadidoAGrupo`** (user\_hrn: "...", group\_hrn: "...")

3.  👤 `Admin` -\> 🟥 **Editor de Políticas Cedar** -\> 🔵 `ActualizarPolíticaCedar` (policy\_id: "...", contenido: `permit(principal, action == Action::"read", resource) when { resource in Repository::"hrn:...:repository/staging-repo" && principal in Group::"hrn:...:group/qa-team" };`)

    * **Decisión**: ¿La sintaxis de la política es válida?
    * 🟠 **`PolíticaCedarActualizada`** (policy\_id: "...")

4.  **(Más tarde)** 👤 `Usuario` (`dev3`) -\> 🔵 `DescargarVersiónDePaquete` (del repo `staging-repo`)

    * **Decisión de Autorización (Cedar)**: El principal (`dev3`) está en el grupo `qa-team`, y el recurso (`artefacto`) está en el repositorio `staging-repo`. **El acceso es permitido.**
    * 🟠 `VersiónDePaqueteDescargada`...

-----

### Flujo 11: Ciclo de Vida del Artefacto (Deprecación y Baneo)

**Actores**: 👤 `Mantenedor de Proyecto`, 👤 `Admin de Seguridad`

#### Escenario A: Deprecación

1.  👤 `Mantenedor` -\> 🔵 `DeprecarVersiónDePaquete` (hrn: "...lib-v1", sucesor\_hrn: "...lib-v2") -\> 🟡 `PackageVersion`

    * **Decisión**: ¿El usuario tiene permisos para deprecar? ¿Existe la versión sucesora?
    * 🟠 **`EstadoDeVersiónDePaqueteCambiado`** (hrn: "...lib-v1", estado\_anterior: Active, estado\_nuevo: Deprecated)

2.  🟢 **Vista `DetalleDeArtefacto` para `lib-v1` ahora muestra:** "⚠️ Esta versión está deprecada. Por favor, use la v2." (con un enlace).

3.  🟢 **Búsquedas de Paquetes** ahora pueden filtrar `is:deprecated`.

#### Escenario B: Baneo

1.  👤 `Admin de Seguridad` (tras un análisis manual) -\> 🔵 `BanearVersiónDePaquete` (hrn: "...malicious-lib", razón: "Contiene malware confirmado") -\> 🟡 `PackageVersion`

    * **Decisión**: ¿El usuario tiene permisos de baneo?
    * 🟠 **`EstadoDeVersiónDePaqueteCambiado`** (hrn: "...malicious-lib", estado\_anterior: Quarantined, estado\_nuevo: Banned)

2.  🟣 **Política de Sistema**: **Siempre que** `EstadoDeVersiónDePaqueteCambiado` a `Banned`...

    * **Entonces** -\> 🔵 `AñadirHashABloqueoGlobal` (content\_hash: "sha256:xyz...")

3.  **(Más tarde)** 👤 `Otro Usuario` -\> 🔵 `PublicarVersiónDePaquete` (con un fichero que tiene el hash "sha256:xyz...")

    * **Decisión**: ¿El hash del artefacto está en la lista de bloqueo global? **Sí**. **El despliegue es rechazado.**
    * 🟠 **`PublicaciónDeArtefactoRechazadaPorBaneo`**

-----

