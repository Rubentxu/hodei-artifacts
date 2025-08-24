# PRD: Sistema de Repositorio de Artefactos de Alto Rendimiento

## Introducción

El presente documento detalla los **requerimientos del producto (PRD)** para un sistema de repositorio de artefactos de alto rendimiento. Este sistema está destinado a almacenar, gestionar y servir artefactos de software (como bibliotecas y paquetes) de forma eficiente y escalable, tomando como referencia las funcionalidades de herramientas existentes como **Sonatype Nexus**, **JFrog Artifactory** o **Apache Archiva**. El objetivo es **replicar y mejorar** dichas funcionalidades, proporcionando una solución moderna con un backend en **Kotlin/Native** y un frontend en **Angular 19+**, siguiendo una arquitectura **Hexagonal** (puertos y adaptadores) y **dirigida por eventos** (Event-Driven).

En esencia, el repositorio de artefactos permitirá a equipos de desarrollo almacenar compilaciones y dependencias en un lugar centralizado, facilitando la distribución interna de componentes y mejorando los ciclos de integración continua. Se busca una plataforma segura, de alto rendimiento y preparada para la nube, que pueda integrarse fácilmente en entornos empresariales modernos y flujos DevOps.

## Objetivos del sistema

* **Alto rendimiento y eficiencia:** Diseñar el sistema para manejar grandes volúmenes de solicitudes de descarga y carga de artefactos con baja latencia. El backend en Kotlin/Native ofrecerá ventajas de rendimiento (memoria y velocidad) al compilarse a código nativo sin la sobrecarga de una JVM. Se priorizará el uso eficiente de recursos y la capacidad de servir artefactos rápidamente incluso bajo alta carga.

* **Escalabilidad horizontal:** Permitir que el sistema escale en entornos **Kubernetes** añadiendo instancias adicionales para distribuir la carga. La arquitectura event-driven ayudará a desacoplar componentes, facilitando la escalabilidad sin cuellos de botella. Se contemplará la posibilidad de despliegue multinodo para alta disponibilidad (HA) y balanceo de carga, soportando escenarios empresariales con miles o millones de solicitudes diarias.

* **Soporte multiplataforma de artefactos:** Brindar soporte inicial para **repositorios Maven (Java)** y **paquetes npm (Node.js)**, cubriendo los dos ecosistemas de dependencias más comunes. El diseño permitirá añadir fácilmente nuevos tipos de artefactos en el futuro (por ejemplo: imágenes Docker, paquetes PyPI de Python, paquetes NuGet de .NET, etc.) sin afectar a la arquitectura central, aprovechando la naturaleza extensible de la arquitectura hexagonal.

* **Seguridad y control de acceso (RBAC):** Implementar un modelo de **Control de Acceso Basado en Roles (RBAC)** robusto, de modo que solo usuarios autenticados con los permisos adecuados puedan subir o descargar artefactos de determinados repositorios. Esto garantiza que los artefactos internos estén protegidos y que se pueda controlar el acceso por proyecto, equipo o entorno. La autenticación será obligatoria para operaciones sensibles (como publicar artefactos), y el sistema soportará roles predefinidos (admin, desarrollador, lector, etc.) así como la capacidad de definir permisos a nivel de repositorio.

* **Flexibilidad de almacenamiento:** Permitir almacenar los archivos binarios de los artefactos tanto en **almacenamiento local** (sistema de ficheros del servidor, con volúmenes persistentes) como en **Amazon S3** u otros almacenamientos de objetos en la nube. Esto brinda opciones de despliegue on-premise y en la nube. El sistema usará el sistema de archivos para guardar los artefactos (binaries) en lugar de una base de datos relacional, ya que guardar archivos directamente en almacenamiento plano ofrece mejor rendimiento para este caso de uso. En modo S3, se aprovechará la escalabilidad y durabilidad del servicio de objetos de AWS, manteniendo metadatos de artefactos de forma consistente con los objetos almacenados.

* **Despliegue Cloud-Native:** Construir el sistema de forma **nativa para contenedores** y listo para Kubernetes. Esto implica proveer definiciones de contenedor (Docker) optimizadas, configuraciones externas (12-factor app), y soporte de *health checks* (sondas de *liveness* y *readiness*) para integrarse con orquestadores. El objetivo es que la instalación y escalado en clusters Kubernetes sea directo, facilitando la integración en pipelines DevOps y entornos híbridos o multi-nube.

* **Calidad mediante TDD:** Adoptar una estrategia de **Desarrollo Guiado por Pruebas (TDD)**, asegurando que cada funcionalidad venga respaldada por pruebas automatizadas desde su concepción. Se usará **Testcontainers** para pruebas de integración, levantando dependencias reales en contenedores efímeros (por ejemplo, una instancia simulada de S3, bases de datos, etc.) de modo que las pruebas cubran escenarios cercanos a producción. La calidad será un pilar fundamental, minimizando regresiones y permitiendo refactors seguros gracias a una suite de pruebas completa.

* **Documentación exhaustiva y automatizada:** Producir documentación técnica detallada tanto para usuarios finales (consumidores del repositorio) como para desarrolladores. Esto incluye una **API OpenAPI/Swagger** completa para el backend (permitiendo explorar los endpoints REST fácilmente) y documentación auto-generada del código y componentes para desarrolladores que trabajen en el proyecto. El objetivo es que cualquier integrante del equipo técnico pueda entender rápidamente el diseño, los endpoints disponibles y cómo extender o usar el sistema. La documentación se mantendrá actualizada de forma continua (por ejemplo, generándola automáticamente en cada build o despliegue).

## Público objetivo

El sistema está dirigido principalmente a **equipos técnicos de desarrollo, DevOps e infraestructura** en organizaciones de software. A continuación se describen los públicos clave:

* **Desarrolladores de software:** Quienes publicarán librerías internas, módulos compartidos y componentes reutilizables en el repositorio para que otros proyectos los consuman. También desarrolladores que consumirán dependencias desde el repositorio central en lugar de repositorios públicos, acelerando builds al aprovechar artefactos locales cacheados.

* **Ingenieros DevOps / Integración Continua:** Responsables de configurar pipelines de CI/CD donde las **builds automáticas publican artefactos** (por ejemplo, JARs, archivos npm) en este repositorio tras compilaciones exitosas, y luego despliegan servicios utilizando esos artefactos. Este público requiere que el sistema se integre fácilmente con herramientas como Jenkins, GitLab CI, GitHub Actions, etc., y que exponga APIs para automatizar la publicación y el versionado de artefactos.

* **Arquitectos de software y líderes técnicos:** Interesados en una solución robusta que garantice la **seguridad del suministro de software** (software supply chain) y la trazabilidad de las dependencias. Para ellos, características como control de acceso, registro de auditoría de descargas/publicaciones, y posibilidad de escanear artefactos en busca de vulnerabilidades en un futuro, serán importantes. También valorarán la arquitectura modular (hexagonal/event-driven) que facilite mantener y escalar el sistema en el largo plazo.

* **Administradores de sistemas/infraestructura:** Encargados de desplegar y operar el sistema de repositorio. Necesitan que sea fácil de desplegar en **Kubernetes** u otras plataformas, que soporte monitoreo, logging centralizado y backups. También se benefician de la capacidad de escalar el sistema para soportar a una organización entera sin degradación de rendimiento.

Adicionalmente, en un contexto más amplio, el sistema podría ser útil para **comunidades open-source o empresas de producto** que necesiten hospedar sus propios paquetes (por motivos de privacidad, costo o rendimiento). Sin embargo, el enfoque inicial está en entornos empresariales internos, donde se requiera un control total sobre los artefactos construidos y usados en la organización.

## Requerimientos funcionales

A continuación se enumeran las funcionalidades clave que el sistema debe proporcionar en su alcance inicial:

* **Soporte para artefactos Maven:** El sistema actuará como un **repositorio Maven** privado. Permitirá publicar (`deploy`) y descargar (`fetch`) artefactos Java (JAR, WAR, archivos `.pom`, etc.) utilizando las herramientas estándares (Maven, Gradle). Deberá respetar la estructura de coordenadas Maven (groupId/artifactId/version) y gestionar tanto versiones de lanzamiento como *snapshots*. También debe calcular y almacenar sumas de verificación (SHA1/MD5) de cada artefacto, asegurando la integridad en las transferencias, tal como lo hacen los repositorios Maven tradicionales. Se espera que los desarrolladores puedan apuntar sus builds a este repositorio (mediante la URL y credenciales) y resolver las dependencias de forma transparente. En futuras iteraciones, se podría soportar funcionalidades avanzadas como **proxy** de repositorios remotos (p.ej. cachear Maven Central), pero inicialmente el enfoque es en repositorios alojados internos.

* **Soporte para paquetes npm:** De igual manera, el sistema ofrecerá las capacidades de un **registro npm** privado. Soportará la publicación de paquetes Node.js (ficheros `.tgz` resultantes de `npm pack`) y las operaciones de instalación (`npm install`) apuntando el registro hacia nuestro sistema. Deberá implementar las APIs necesarias que espera el cliente npm: por ejemplo, endpoints para obtener el *metadata* de un paquete (sus versiones, dependencias), para publicar nuevas versiones y para descargar los tarballs. El sistema manejará autenticación de npm (tokens de acceso o a través de autenticación básica) integrándose con el modelo RBAC definido. Esto permitirá a los equipos JavaScript/TypeScript tener un registro local para sus módulos, reduciendo dependencia de la nube y acelerando las instalaciones de paquetes comunes en la organización.

* **Gestión de repositorios y artefactos:** A través de la interfaz administrativa (web UI) o APIs, los usuarios con rol adecuado (ej. administradores) podrán **crear y configurar repositorios** de artefactos. Por ejemplo, se podrá definir un repositorio Maven para "librerías internas" y otro separado para "snapshots" de desarrollo, o un repositorio npm para cada equipo/proyecto si se requiere aislamiento. Las configuraciones incluyen: nombre del repositorio, tipo (Maven, npm), políticas de retención (p.ej. mantener solo X últimas versiones de snapshots), y selección del backend de almacenamiento (local o S3) si aplica. Asimismo, la herramienta permitirá **navegar por los artefactos** almacenados: listar grupos, artefactos y versiones en Maven; listar paquetes y versiones en npm. Los usuarios podrán buscar artefactos por nombre o coordenadas de forma básica (consulta por prefijo, coincidencia exacta, etc., inicialmente enfocándonos en búsquedas simples). Cada artefacto tendrá una vista de detalles con su metadata (por ejemplo, POM info, tamaño, checksum, fecha de publicación, quién lo publicó). Es necesario incluir también la funcionalidad de **eliminar** artefactos o versiones (especialmente para snapshots o paquetes npm deprecated), operación que estará restringida a roles autorizados. Todas las acciones de publicación, descarga y borrado deberían registrarse para auditoría.

* **Autenticación y autorización (RBAC):** Implementar un sistema de autenticación de usuarios robusto. Inicialmente se puede usar autenticación mediante usuario/contraseña (almacenados de forma segura con hashing) con la posibilidad de generar **tokens de acceso** para CI/CD u otras herramientas no interactivas. Alternativamente o complementariamente, se podría soportar autenticación mediante tokens npm y tokens Maven (como los **tokens Bearer** de npm, o la utilización de servidores de autenticación externos en el futuro). Una vez autenticados, el sistema aplicará **Control de Acceso Basado en Roles (RBAC)**. Se definirán roles predeterminados:

    * *Administrador:* acceso completo a todas las funciones, puede gestionar usuarios, roles, repositorios y artefactos.
    * *Desarrollador/Mantenedor:* puede publicar artefactos en repositorios específicos asignados, y leer/descargar artefactos.
    * *Lector:* solo puede leer/descargar artefactos de repositorios permitidos, sin capacidad de publicación.
    * (Opcionalmente, roles separados por tipo de repo o proyecto, según se configuren).

  La asignación de permisos podrá ser a nivel de repositorio: p. ej., un usuario puede tener rol de mantenedor en un repositorio (puede publicar ahí) pero solo lector en otro. El modelo RBAC debe ser flexible para adaptarse a políticas de organizaciones. Todas las rutas sensibles del backend (ej. subir artefacto, borrar, crear repos) estarán protegidas para que solo roles válidos accedan. Se incorporará también la opción de **anónimo de solo lectura** para ciertos repositorios si la empresa desea distribuir ciertos artefactos públicamente (configurable, por defecto todo requiere auth). En cuanto a implementación, tras login se usará probablemente JWT u otro mecanismo para que las siguientes peticiones REST incluyan un token de autenticación.

* **Interfaz de usuario web (Frontend Angular):** Desarrollar una aplicación web amigable en **Angular 19+** que permita interactuar con el repositorio de artefactos de forma visual. Esta UI servirá tanto a administradores como a desarrolladores. Entre las funcionalidades de la interfaz:

    * **Dashboard/Resumen:** vista general del sistema (espacio utilizado, número de artefactos, últimas publicaciones, etc.).
    * **Exploración de repositorios:** listas y árboles de navegación para repositorios Maven (por groupId/artifactId) y listados de paquetes npm, permitiendo filtrado por nombre.
    * **Detalle de artefacto/paquete:** mostrando información de la versión seleccionada (metadatos, archivos, comandos para usarlo en Maven/npm, historial de versiones).
    * **Carga/Publicación manual:** aunque la mayoría de publicaciones serán vía herramientas de build, la UI podría ofrecer una manera de subir un artefacto manualmente (por ejemplo, un formulario donde un admin puede subir un bundle npm o un JAR, asociándolo a un grupo/version específicos).
    * **Gestión de usuarios y roles:** formularios para crear usuarios, asignar roles y permisos a repositorios.
    * **Configuración de repositorios:** crear/editar/eliminar repos, configurar sus opciones (por ejemplo, toggle si usar almacenamiento S3 o local para ese repo, políticas de expiración de snapshots, habilitar anonimato de lectura, etc.).
    * **Historial y logs:** ver un registro de acciones recientes (últimos artefactos subidos, usuarios registrados, intentos fallidos de login, etc.), para monitorear el uso.

  La aplicación frontend debe ser **responsive** (accesible desde navegadores en distintas resoluciones) y proporcionar una experiencia fluida. Se utilizarán las mejores prácticas de Angular (lazy loading de módulos, componentes reutilizables, manejo de estado con RxJS o NgRx si aplica para datos compartidos, etc.). También se integrarán notificaciones en tiempo real en la UI para ciertas acciones si es posible (por ejemplo, usando WebSockets o EventSource en el futuro para avisar de nuevos artefactos sin refrescar la página, aunque inicialmente podría bastar con refresco manual o polling ligero). La UI consumirá las APIs REST del backend (documentadas con OpenAPI), garantizando que cualquier funcionalidad disponible vía API esté accesible también vía la interfaz gráfica.

* **API REST y automatización:** Todo el núcleo funcional estará expuesto mediante **APIs RESTful** bien definidas, de tal forma que la interacción con el sistema pueda ser automatizada sin usar la interfaz gráfica. Esto incluye:

    * API para publicación de artefactos Maven (p.ej. un endpoint `PUT /maven/{repo}/{groupId}/{artifactId}/{version}` que reciba el artifact y su POM).
    * API para publicar paquetes npm (por ejemplo, endpoints equivalentes a los del registro npm: `PUT /npm/{repo}/` para subir, etc.).
    * APIs para descargar artefactos (`GET` a rutas de artefactos, con soporte de autenticación y redirección a S3 si es el caso).
    * API de gestión: creación de repos (`POST /repos`), listado de repos (`GET /repos`), crear usuario (`POST /users`), asignar rol (`PUT /users/{user}/roles`), etc.

  Todas estas APIs estarán documentadas con **OpenAPI/Swagger**, facilitando su uso desde scripts o integraciones personalizadas. Por ejemplo, un script de Jenkins podría pegarle al endpoint de publicación con un token para subir el resultado de una build. La API seguirá principios REST claros, utilizando códigos de respuesta estándar (201 Created al subir, 401 Unauthorized sin auth, etc.) y retornando mensajes de error descriptivos en formato JSON. La **integración con CI/CD** será clave: se buscará que el sistema sea fácilmente utilizable en pipelines automatizados – por ejemplo, proporcionando snippets de configuración (Maven `settings.xml` con credenciales, o `.npmrc`) para apuntar las herramientas al nuevo repositorio interno.

En resumen, los requerimientos funcionales aseguran que el sistema cubra las operaciones esenciales de un gestor de artefactos (publicar, almacenar, obtener, organizar, asegurar el acceso y proveer visibilidad vía UI), alineándose con lo que usuarios esperan de herramientas como Nexus o Artifactory, pero construidas con tecnología y enfoque modernos.

## Requerimientos no funcionales

Además de las funciones anteriores, el sistema debe cumplir con una serie de requisitos no funcionales que garanticen su calidad, rendimiento y mantenibilidad:

* **Rendimiento y concurrencia:** El repositorio debe poder servir archivos grandes (por ejemplo JARs de decenas de MB, tarballs npm, etc.) de forma eficiente, optimizando el streaming de datos para no consumir memoria excesiva. Se usará E/S no bloqueante y mecanismos de **streaming** para enviar artefactos al cliente mientras se leen del almacenamiento, reduciendo la latencia. El backend en Kotlin con soporte nativo permitirá un rendimiento cercano al de lenguajes compilados, y las pruebas de carga deben demostrar que el sistema soporta altas tasas de transferencia (p. ej., decenas de solicitudes concurrentes de artefactos de 100MB sin degradación significativa). Se establecerán métricas objetivo, como por ejemplo: capacidad de manejar >1000 solicitudes de descarga por segundo de artefactos pequeños, o saturar un enlace de red gigabit sirviendo artefactos grandes sin que el CPU sea el cuello de botella. Asimismo, la latencia de las operaciones de metadata (listar repos, obtener info de artifact) deberá ser baja (milisegundos), apoyándose en cache en memoria de metadatos frecuentemente consultados si es necesario.

* **Escalabilidad horizontal y vertical:** El diseño debe permitir tanto escalar verticalmente (mejor hardware, más CPU/RAM por instancia) como horizontalmente (más instancias en cluster). En Kubernetes, se debe poder ejecutar varias réplicas del servicio backend detrás de un Service/Ingress, sirviendo tráfico en balanceo. Para lograr consistencia en un entorno distribuido:

    * Los artefactos en almacenamiento compartido (S3) aseguran que cualquier réplica pueda atender las mismas solicitudes de descarga. En caso de almacenamiento local, se recomendará usar un volumen compartido (NFS, GlusterFS, etc.) o dedicar ciertos repos a instancias específicas (aunque S3 es la opción preferida para escalado).
    * Los metadatos (usuarios, permisos, índices de artefactos) residirán en una base de datos común accesible por todas las instancias, evitando divergencia. Podría utilizarse una base de datos relacional central (por ejemplo PostgreSQL) para este propósito – de hecho, referencias de Nexus muestran que para cargas altas es recomendado usar PostgreSQL en lugar de un motor embebido.
    * La arquitectura hexagonal facilita escalabilidad al permitir reemplazar componentes (por ejemplo, cambiar de una implementación local a un servicio distribuido de cache sin alterar la lógica central).

  Adicionalmente, se considerará el uso de **caching** distribuido: por ejemplo, los bytes de artefactos más solicitados podrían cachearse en memoria o en un CDN front-end para aliviar carga del backend. El sistema debería integrarse bien con CDNs o proxies inversos (por ejemplo, se puede poner Nginx/Apache para servir contenidos estáticos de ciertos repositorios públicos si fuera necesario).

* **Disponibilidad y tolerancia a fallos:** Dado que este repositorio puede convertirse en una pieza crítica de la infraestructura de build, debe ser altamente disponible. En un despliegue multi-nodo, se buscará **eliminación de single points of failure**: múltiples instancias de aplicación, base de datos con replicación o clúster (si es PostgreSQL, configurarlo en HA), y almacenamiento S3 altamente disponible. En Kubernetes, definir *readiness probes* para retirar del servicio a instancias no saludables, y posiblemente auto-escalar (horizontal pod autoscaler) en función de carga. Se prevé soportar implementaciones de **failover** – por ejemplo, backups y restauración de la base de datos, o replicación geográfica si el repositorio se usa en varias regiones. Los upgrades de versión del sistema deben poder hacerse con **cero downtime** (desplegando en rolling update gracias a que múltiples instancias pueden convivir temporalmente). También es importante la **consistencia de datos**: se asegurará que las operaciones críticas (como publicar un artefacto) sean atómicas – es decir, que un artefacto solo se marque disponible cuando tanto su archivo como sus metadatos estén almacenados correctamente. Se manejarán transacciones en la medida necesaria (por ejemplo, primero subir el archivo a S3, luego registrar metadatos en DB; si algo falla, revertir y limpiar para no dejar artefactos huérfanos).

* **Seguridad (no funcional):** Más allá del control de acceso funcional (RBAC), el sistema debe ser seguro en términos de implementación:

    * Todos los **datos sensibles** (contraseñas de usuarios, tokens) se almacenarán cifrados/hasheados con algoritmos fuertes (bcrypt o argon2 para contraseñas).
    * La comunicación con el repositorio deberá realizarse preferentemente sobre **HTTPS** (TLS), evitando filtraciones de credenciales o artefactos. Aunque TLS suele ser manejado por la capa de ingress (por ejemplo, un ingress controller en K8s), el sistema proveerá configuraciones para habilitar TLS directamente si se despliega standalone.
    * Protección contra ataques comunes: validar inputs para prevenir SQL injection (usando ORMs o consultas parametrizadas), proteger la API de **CSRF** en caso de uso desde el navegador (con tokens anti-CSRF en la UI Angular si se usan cookies de sesión, aunque usando JWT Bearer no habría sesión en servidor), y evitar **XSS** asegurando que la UI maneje correctamente la escape de contenidos dinámicos. Angular de por sí ayuda con XSS al escapar expresiones, pero se debe revisar cualquier inserción directa de HTML.
    * **Registro de auditoría:** las acciones importantes (login, intentos fallidos, subida/borrado de artefactos, cambios de configuración) quedarán registradas en logs auditables. Esto es crucial para investigar incidentes de seguridad o cumplimiento normativo.
    * **Políticas de contraseña y rotación de tokens:** se podrán establecer requisitos mínimos (longitud, complejidad) para contraseñas de usuarios, y los tokens de acceso tendrán expiración configurable, reduciendo ventana de compromiso en caso de filtración.
    * **Aislamiento y sandboxing:** El sistema se ejecutará en contenedores con permisos acotados (usuario no root en el contenedor, solo puertos necesarios abiertos) y se recomienda desplegar en redes seguras. Además, los artefactos subidos serán tratados como datos opacos (no se ejecutan ni se descomprimen en servidor salvo necesidad de extraer metadata), minimizando riesgo de carga maliciosa.

* **Compatibilidad e integraciones:** Aunque inicialmente se limita a Maven/npm, se considera importante que el diseño no excluya **formatos adicionales**. Por ejemplo, permitir que en el futuro un repositorio pueda soportar **Docker images (OCI)**, lo cual implicaría manejar capas de imágenes y manifiestos; o **paquetes PyPI**, etc. La arquitectura hexagonal ayudará a esto al permitir agregar nuevos adaptadores para manejar protocolos distintos. También, pensando en integraciones empresariales, sería deseable en el futuro soportar integración con **LDAP/Active Directory** para la autenticación (de modo que los usuarios corporativos puedan reutilizar sus credenciales), o **OIDC/SAML** para SSO; aunque esto no es parte del alcance inmediato, la modularidad en auth debe dejar espacio para ello. Igualmente, se prevé integrar escáneres de seguridad de artefactos o **firmado de artefactos** (por ejemplo, validar firmas GPG en JARs) más adelante, contribuyendo a un ecosistema de DevSecOps.

* **Mantenibilidad y extensibilidad:** Dado que el sistema será desarrollado y mantenido por un equipo técnico, se enfatiza la escritura de un código limpio, modular y con buenas prácticas. La arquitectura hexagonal fomenta la mantenibilidad al separar claramente las responsabilidades; los casos de uso del dominio (p.ej. *"publicar artefacto"*, *"descargar artefacto"*) estarán desacoplados de detalles de infraestructura (almacenamiento, DB, red) facilitando que los desarrolladores entiendan la lógica de negocio sin distraerse con detalles técnicos. Se adoptarán convenios de estilo (Kotlin y TypeScript) y se realizará **revisión de código** para asegurar consistencia. Además, la extensibilidad implica que agregar una nueva funcionalidad o soporte a un nuevo tipo de repositorio no requiera reescribir todo: por ejemplo, si se añade soporte a **NuGet** (packages .NET), se creará un nuevo módulo adaptador que implementa las interfaces necesarias, sin cambiar las capas internas. Documentación actualizada y una suite de pruebas robusta complementan la mantenibilidad al permitir comprender y modificar el sistema con confianza.

* **Registro y monitoreo:** Desde el inicio, se incluirán capacidades de **logging** y **monitoring**. Todos los componentes clave registrarán eventos importantes (en formato estructurado JSON si posible, para facilitar ingestión en herramientas tipo ELK stack). Se expondrán métricas de aplicación (ej: número de descargas, tiempos de respuesta, conteo de artefactos) a través de, por ejemplo, un endpoint **/metrics** compatible con **Prometheus**, permitiendo que el sistema sea monitoreado en entornos Kubernetes fácilmente. También se pueden generar trazas distribuidas si se incorporan eventos u operaciones que atraviesan varias capas (considerar OpenTelemetry en el futuro). Estas características no son funcionalidades visibles al usuario final pero sí cruciales para la operación en producción y para diagnosticar problemas de rendimiento o errores.

En conjunto, estos requerimientos no funcionales garantizan que el producto no solo haga lo que debe hacer, sino que lo haga de forma **rápida, confiable, segura y mantenible**, preparado para su uso en entornos productivos exigentes.

## Diseño de arquitectura (Hexagonal y Event-Driven)

En esta sección se describe la arquitectura propuesta para el sistema, que combina los principios de **Arquitectura Hexagonal (Ports & Adapters)** con un enfoque **Dirigido por Eventos** para lograr un diseño desacoplado, flexible y escalable. La elección de arquitectura es clave para cumplir los objetivos de extensibilidad y rendimiento mencionados.

### Arquitectura Hexagonal (Puertos y Adaptadores)

La arquitectura hexagonal propone aislar el núcleo de la aplicación (lógica de negocio o dominio) de los detalles de infraestructura, utilizando **puertos** (interfaces) y **adaptadores** (implementaciones) para conectar el dominio con el mundo exterior. En nuestro contexto, esto se materializa de la siguiente forma:

* **Núcleo de Dominio:** Aquí reside la lógica central del repositorio de artefactos, independiente de frameworks o detalles técnicos. Incluye casos de uso como *PublicarArtefacto*, *ObtenerArtefacto*, *AutenticarUsuario*, *RegistrarUsuario*, etc., y entidades de dominio como *Artefacto*, *Repositorio*, *Usuario*, *Rol*. Por ejemplo, la lógica para validar que un artefacto nuevo cumple ciertos criterios, o para determinar qué permisos requiere una operación, estará en esta capa. El dominio define **puertos (interfaces)** que representan operaciones necesitadas hacia afuera, por ejemplo: un puerto de *AlmacenamientoArtefactos* (con métodos como `guardarArchivo()` y `obtenerArchivo()`), un puerto de *RepositorioUsuarios* (para consultar/almacenar datos de usuarios), puertos de *Notificación/Eventos* (para publicar eventos de negocio), etc. El dominio no sabe nada de *cómo* esas operaciones se realizan, solo las declara.

* **Adaptadores de Entrada (Driving Adapters):** Son las piezas que **invocan al dominio** desde el exterior. En nuestro sistema habrá principalmente dos adaptadores de entrada:

    * La **API REST** (controladores HTTP) que recibe las peticiones de clientes (ya sea la UI Angular o herramientas como Maven/npm) y las traduce a llamadas al dominio. Por ejemplo, un controlador REST recibiría `POST /maven/{repo}` con un archivo, extraería los datos necesarios y llamaría al caso de uso *PublicarArtefacto* del dominio.
    * Eventualmente otros adaptadores de entrada podrían ser líneas de comando (CLI) u interfaces de mensajería, pero inicialmente el enfoque principal es REST/HTTP.

  Estos adaptadores de entrada se implementarán posiblemente usando el framework web elegido (Ktor en Kotlin, ver detalles técnicos más abajo), manejando la serialización JSON, validación de input, autenticación (p. ej. interceptores que validen JWT/token y obtengan el Usuario) y luego delegando al servicio de dominio correspondiente.

* **Adaptadores de Salida (Driven Adapters):** Son las implementaciones concretas de los puertos que el dominio utiliza para interactuar con sistemas externos. En nuestro diseño, habrá varios adaptadores de salida, por ejemplo:

    * Adaptador de **almacenamiento local** y adaptador de **almacenamiento S3**: ambos implementan la interfaz *AlmacenamientoArtefactos*. El de almacenamiento local usará el sistema de ficheros (por ejemplo, escribiendo en un directorio específico del servidor o volumen montado) para guardar los archivos de artefactos, mientras que el adaptador S3 se comunicará con AWS S3 (usando la AWS SDK) para subir/descargar objetos desde un bucket configurado. El dominio no distingue cuál se usa; mediante configuración se elegirá uno u otro por repositorio o globalmente.
    * Adaptador de **base de datos de metadatos**: implementa puertos como *RepositorioUsuarios* (posiblemente con un ORM o consultas SQL a una base relacional) y quizá *RepositorioArtefactosMetadata* (para información textual de los artefactos, si almacenamos detalles adicionales como descripciones, dependencias, etc., aparte de los binarios). Por ejemplo, podríamos tener una tabla de usuarios, tabla de roles, tabla de asignaciones, y tablas de artefactos (coordenadas, checksums, ubicación del archivo, etc.).
    * Adaptador de **notificación/eventos**: para implementar un puerto de publicación de eventos de dominio. En una versión simple, podría ser un componente que simplemente escribe logs o envía eventos internos in-process; en una versión más distribuida, podría publicar mensajes en un broker externo (Kafka, RabbitMQ, etc.). Se detalla más en la sección Event-Driven.
    * Adaptador de **servicios externos** (si los hubiera): por ejemplo, si integráramos un servicio de escaneo de vulnerabilidades, habría un adaptador que llama a ese servicio cuando se sube un nuevo artefacto (implementando un puerto *EscaneoArtefactos* definido en el dominio).

* **Comunicación entre capas:** En la arquitectura hexagonal, las dependencias apuntan *hacia adentro* – es decir, los adaptadores conocen/llaman al dominio, pero el dominio no conoce a los adaptadores concretos. Esto se logra mediante inyección de dependencias o pasándoles instancias de interfaces. Por ejemplo, el caso de uso *PublicarArtefacto* podría recibir en su constructor (o como parámetros) un `AlmacenamientoArtefactos` y un `RepositorioArtefactosMetadata`. Al ejecutar, usará esos puertos, sin saber si detrás hay S3, disco local o una simulación en memoria (lo que facilita pruebas unitarias usando stubs/mocks). Esta separación asegura un **bajo acoplamiento** y que componentes sean **intercambiables** en cualquier nivel (por ejemplo, podríamos reemplazar la base de datos relacional por otra tecnología NoSQL en el futuro implementando el mismo puerto, sin modificar la lógica central).

* **Facilitando pruebas y mantenimiento:** Gracias a esta arquitectura, la mayoría de la lógica de negocio se puede probar aisladamente (*testing* del dominio usando dobles para los puertos). Además, añadir un nuevo tipo de artefacto (p.ej. repositorio PyPI) implicaría agregar nuevos adaptadores de entrada (endpoints específicos, parsing de paquetes Python) y quizás un nuevo puerto si requiere distinto manejo de metadata, pero **no rompe** lo existente. De igual forma, cambiar el mecanismo de almacenamiento (p. ej. cambiar S3 por Azure Blob Storage) solo requiere un nuevo adaptador de salida que implemente el puerto de storage. Esta modularidad nos permitirá evolucionar el producto en el tiempo con menos riesgo.

En resumen, la arquitectura hexagonal nos proporciona un marco para construir un sistema **modular, intercambiable y enfocado en el dominio**. Los componentes de la aplicación estarán bien aislados, favoreciendo tanto la extensibilidad técnica (nuevas integraciones) como la claridad en el diseño.

### Arquitectura dirigida por eventos (Event-Driven)

Además del esquema de puertos y adaptadores, el sistema incorporará un enfoque **event-driven**, lo que significa que reaccionará a eventos de negocio y utilizará la mensajería asíncrona para desacoplar procesos. En una **Arquitectura Dirigida por Eventos**, los componentes generan eventos cuando ocurre algo relevante, y otros componentes los consumen sin que el emisor necesite conocer a los receptores. Aplicado a nuestro sistema:

* **Eventos de dominio:** Identificaremos ciertos **eventos clave** en el ciclo de vida del repositorio, tales como `ArtefactoPublicado`, `ArtefactoDescargado`, `NuevoUsuarioCreado`, `RepositorioCreado`, etc. Cuando la lógica de negocio complete una acción importante, publicará un evento correspondiente. Por ejemplo, después de que un artefacto se almacena exitosamente, el caso de uso *PublicarArtefacto* puede generar un evento `ArtefactoPublicado` con detalles como el identificador del artefacto, versión, quién lo subió y timestamp.

* **Manejadores de eventos internos:** Otros componentes del mismo sistema (o módulos separados) pueden **suscribirse** a esos eventos para realizar tareas adicionales de manera asíncrona. Por ejemplo:

    * Tras un `ArtefactoPublicado`, un manejador podría actualizar un índice de búsqueda (si implementamos búsqueda avanzada) en segundo plano, en lugar de hacerlo sincrónicamente en la petición de subida.
    * Otro manejador podría enviar una notificación (email o webhook) a interesados indicando que hay una nueva versión disponible.
    * Tras un evento `ArtefactoDescargado`, podríamos acumular métricas de uso (contador de descargas) sin ralentizar la entrega del archivo al usuario.
    * Un evento `NuevoUsuarioCreado` podría ser consumido para, por ejemplo, enviar un correo de bienvenida o registrar la creación en un sistema central de auditoría corporativa.

  Gracias a esto, el sistema principal no se bloquea realizando todas esas acciones en línea, sino que las delega a procesos asíncronos, mejorando la capacidad de respuesta y la **flexibilidad para agregar comportamientos adicionales** sin modificar la lógica central.

* **Infraestructura de eventos:** Para implementar la comunicación de eventos tenemos opciones. En una primera versión (sencilla, monolítica), podríamos emplear un **bus de eventos en memoria** – básicamente, un mecanismo donde los diferentes componentes registran callbacks para ciertos eventos, y cuando se emite el evento, se invocan esos callbacks. Kotlin puede manejar esto con coroutines/flows, o mediante librerías de eventos. Esto ya provee los beneficios de desacoplamiento dentro de una sola instancia de la aplicación.

  Sin embargo, dado que apuntamos a escalabilidad con múltiples instancias, es probable que consideremos una solución más robusta como un **broker de mensajería** externo:

    * Integrar un sistema de mensajería tipo **Apache Kafka** o **RabbitMQ**. En este modelo, cuando ocurre un evento, el adaptador correspondiente publica un mensaje en un *topic* (por ejemplo, "artefactos.publicados"). Una o varias instancias (o servicios separados) suscritas a ese topic reciben el evento y ejecutan lógica de manejo.
    * Kafka en particular se alinea con arquitecturas event-driven escalables, permitiendo reintentos, persistencia de eventos, y procesamientos por lotes si fuese necesario. No obstante, Kafka añade complejidad operativa, por lo que podríamos hacerlo opcional/modular: es decir, el sistema podría operar sin Kafka (manejando eventos internamente), pero si se despliega en una organización que ya tiene Kafka, se podría activar un adaptador Kafka para distribuir eventos entre instancias o incluso integrar con otros sistemas corporativos.
    * RabbitMQ u otros brokers AMQP podrían usarse si se prefiere un enfoque de colas de trabajo para ciertas tareas.

* **Modelo de suscripción:** La arquitectura event-driven requerirá definir cómo se configuran los suscriptores. En términos hexagonales, podríamos tener un puerto de *PublicaciónEventos* con implementaciones (adaptadores) para distintos medios (in-memory vs Kafka). Los manejadores de eventos podrían residir en el propio servicio (por ejemplo, un componente que al inicializar se suscribe al bus de eventos in-memory o a un tópico Kafka). Alternativamente, podríamos externalizar ciertos manejadores como microservicios independientes que consuman los eventos (por ejemplo, un microservicio dedicado a indexar artefactos para búsqueda avanzada podría suscribirse a `ArtefactoPublicado`). El diseño debe permitir ambas posibilidades. Inicialmente, podríamos implementar los manejadores dentro del mismo despliegue para simplicidad, pero separando su lógica de manera que luego puedan extraerse si se desea escalar esa parte independientemente.

* **Ventajas de la EDA (Event-Driven Architecture):** Aplicar event-driven nos ofrece **desacoplamiento temporal y funcional** – el emisor no espera a que el trabajo extra se complete, y ni siquiera necesita saber si hay algún interesado en el evento. Esto mejora la robustez: si, por ejemplo, la indexación de búsqueda falla o se ralentiza, no afecta la operación de carga de artefactos en sí (solo quizás ese artefacto no aparezca en resultados de búsqueda hasta reintentar). También permite **extensibilidad**: si en el futuro queremos agregar una nueva reacción a cierto evento, se hace añadiendo un suscriptor nuevo, sin tocar el código existente del emisor. En general, obtenemos una arquitectura más **flexible y reactiva**, capaz de procesar eventos en tiempo real. Un beneficio adicional es que, de cara a integraciones, podemos exponer ciertos eventos a sistemas externos: por ejemplo, publicar en un topic compartido un evento `ArtefactoPublicado` podría permitir que otra aplicación de la empresa (digamos, un registro de cambios, o un sistema de calidad) se entere y actúe.

* **Idempotencia y orden:** Con eventos asíncronos hay que considerar la idempotencia (que si se procesa dos veces no cause efectos no deseados) y el orden. Usando Kafka, los eventos dentro de una misma partición conservarán orden de emisión, pero entre diferentes tipos no necesariamente. Para nuestro caso, la mayoría de eventos son independientes; aún así, diseñaremos los manejadores de forma que puedan tolerar duplicados (por ejemplo, si un mismo `ArtefactoPublicado` se procesara dos veces para indexación, la segunda vez debería reconocer "ya indexado" y no duplicar entrada). Estos detalles se documentarán en la estrategia de implementación de eventos.

En suma, la combinación de arquitectura hexagonal con un enfoque dirigido por eventos nos dará un sistema **altamente modular y reactivo**. La hexagonal asegura límites claros entre la lógica y la tecnología, mientras que los eventos garantizan que el sistema pueda crecer en funcionalidades de manera desacoplada y manejar carga variable distribuyendo trabajo. Este diseño nos posiciona bien para cumplir los requisitos de eficiencia y escalabilidad planteados, a la vez que mantenemos un código organizado.

## Especificaciones técnicas del Backend

El backend será desarrollado en **Kotlin**, haciendo uso de **Kotlin/Native** para compilar a código nativo y maximizar el rendimiento. A continuación se detallan aspectos técnicos clave de esta elección y cómo se implementarán los distintos requisitos en el servidor:

* **Framework web y stack de Kotlin:** Se optará por un framework ligero y eficiente como **Ktor** (framework asíncrono de Kotlin) para construir las APIs REST. Ktor es altamente compatible con corrutinas de Kotlin, lo que permite manejar operaciones de I/O (red, disco) de forma no bloqueante y escalable con carga concurrente. Dado que Kotlin/Native puede no soportar todas las librerías del ecosistema JVM, evaluaremos la posibilidad de usar Kotlin/JVM con GraalVM Native Image para compilar a binario nativo, o directamente Ktor con Kotlin/Native si el soporte es estable. La ventaja de un binario nativo es un arranque mucho más rápido, menor consumo de memoria y ausencia de *garbage collector* pesado, acercando el desempeño a lenguajes como Go o C++. En caso de obstáculos técnicos con Kotlin/Native puro, la alternativa sería usar **GraalVM** para AOT compilar la aplicación Kotlin (JVM) a un ejecutable nativo, logrando similares beneficios.

* **Concurrencia y manejo de I/O:** El servidor utilizará intensivamente las **coroutines** de Kotlin para gestionar concurrencia de forma eficiente. Cada request HTTP entrante se manejará en una coroutine, y las operaciones como lecturas/escrituras de archivos o llamadas a S3 se suspenderán en lugar de bloquear hilos, permitiendo que un número pequeño de hilos sirva a miles de peticiones concurrentes. Esto mejora la escalabilidad en comparación con modelos tradicionales bloqueantes. Además, Ktor soporta pipelines de interceptores que usaremos para ciertas tareas transversales: por ejemplo, un interceptor de autenticación que valide tokens JWT en los headers y rechace peticiones no autorizadas antes de llegar al controlador; o un interceptor de logging para registrar cada request/response.

* **Acceso a datos y almacenamiento:**

    * Para los **metadatos y datos relacionales** (usuarios, roles, referencias de artefactos), integraremos una base de datos. Se podría empezar con una base **embebida** (H2 o SQLite) para desarrollo y pruebas sencillas, pero en producción se recomienda **PostgreSQL** u otro motor robusto, especialmente si se anticipa múltiples instancias del app. Usaremos un **ORM ligero** o una librería de manejo de SQL en Kotlin como Exposed o JOOQ (adaptada a Kotlin) para interactuar con la base. Esto agiliza el desarrollo y reduce riesgo de inyecciones. Las migraciones de esquema se controlarán con Liquibase o Flyway para mantener la estructura de la DB versionada.
    * Para el **almacenamiento de artefactos (binaries)**, tal como se indicó en requisitos, implementaremos una interfaz de repositorio de archivos. En caso de almacenamiento local, bastará con utilizar las API de ficheros de Kotlin/Java (por ejemplo `java.nio.file` via Kotlin) para mover streams de bytes al disco. Se designará una estructura de directorios probablemente por repositorio, y dentro de este replicando la convención de cada formato (p.ej., para Maven, una ruta `<repo>/com/grupo/artifact/1.0/artifact-1.0.jar`). En caso de almacenamiento S3, utilizaremos el **AWS SDK for Kotlin** (si disponible, o invocar REST API de S3 directamente) para subir y bajar objetos. Para eficiencia, podemos usar upload/download en streaming (multipart upload for large files). El adaptador S3 deberá manejar credenciales (posiblemente a través de IAM roles en AWS si se despliega allí, o por configuración de access key/secret). Notar que S3 tiene ligera latencia adicional, por lo que podríamos considerar un caché local de artefactos recientemente solicitados en cada instancia para reducir re-descargas frecuentes, aunque inicialmente podría no ser crítico.
    * Importante: se mantendrá **consistencia** entre la DB y el almacenamiento de archivos. Cada artefacto registrado en DB tendrá campos para su ubicación (p.ej. bucket y key si S3, o ruta si local), tamaño, checksum, etc. Al servir una descarga, el backend consultará DB para encontrar el objeto y luego lo leerá del FS o generará una URL pre-firmada de S3 para redirección (una posible optimización: en vez de hacer streaming via la app, podríamos redirigir al cliente directamente a S3 con un link temporal, ahorrando ancho de banda del servidor, pero esto solo aplica para S3 and if security allows).

* **API RESTful:** Usando Ktor definiremos rutas para los endpoints requeridos. Aprovecharemos la integración con **OpenAPI** (por ejemplo, librerías como kotlindoc or ktor-swagger) para generar la especificación de la API a partir de las definiciones de rutas y modelos. Cada endpoint estará asociado a una función controladora que:

    * Verifique autenticación/autorización (p. ej., comprobar que el usuario tenga rol de *deploy* en el repo destino antes de aceptar un PUT de artefacto).
    * Valide datos de entrada (por ejemplo, nombre de paquete, versión con formato correcto, tamaño no excesivo si ponemos límite).
    * Llame al servicio de dominio correspondiente. Aquí es donde se ve la arquitectura hexagonal en código: el controlador hace algo como `artefactoService.publicarArtefacto(repoId, coordenadas, archivoStream, usuario)`. Ese `artefactoService` pertenece al dominio y encapsula la lógica, interactuando con los puertos necesarios.
    * Devuelva la respuesta adecuada (201 Created, con alguna URL de referencia o ubicación; o el archivo binario en caso de GET).

  Se prestará atención a implementar correctamente semántica HTTP (códigos, métodos idempotentes, uso de ETag y caché HTTP si es aplicable – por ejemplo, podríamos proveer ETag basado en checksum para que clientes no re-descarguen si ya tienen el artefacto). En el caso de npm, también habrá endpoints para las partes específicas de su API (como manejo de tarball downloads via URLs dentro del JSON de package metadata, etc.).

* **Autenticación y sesiones:** Optaremos por **JWT (JSON Web Tokens)** para las sesiones sin estado. Es decir, cuando un usuario inicie sesión (vía un endpoint `/auth/login` enviando credenciales), el servidor validará usuario/contraseña contra la DB y de ser correcto emitirá un JWT firmado que incluye la identidad y roles del usuario. Ese token se devolverá al cliente (la UI lo guardará probablemente en memoria o LocalStorage, y las herramientas automatizadas lo pondrán en header Authorization). En cada petición subsecuente, un **middleware** autenticará el JWT (verificando firma y expiración) y si es válido derivará un objeto usuario/roles disponible para los controladores. De esta forma evitamos guardar sesión en servidor (lo cual facilita escalado). Para la autenticación de npm, que tradicionalmente usa un token en .npmrc, podremos hacer que ese token sea el mismo JWT o tener un tipo de token específico de API pero la mecánica interna será similar. También se considerará un endpoint para **renovación de token** (refresh token) o simplemente solicitar re-login tras expiración, según la política que definamos (por seguridad, tokens JWT no deberían ser de duración excesivamente larga a menos que sean revocados vía algún mecanismo).

  Asimismo, el backend incluirá APIs para manejo de usuarios: crear nuevos usuarios (admin), asignar roles, etc., como se mencionó en funcionales, y estas obviamente tienen que integrarse con el modelo auth (solo un admin autenticado puede crear otro usuario, etc.).

* **Módulo de autorización (RBAC):** Internamente, implementaremos un componente que evalúa permisos. Podría ser tan simple como tener en la tabla de usuarios un campo de rol global, y otra tabla que asocie usuarios/repositorios con permisos específicos. Al entrar una petición, después de determinar el usuario, se consultará este módulo: por ejemplo, si un usuario U solicita PUT en repo "X", se chequea si U tiene permiso de escritura en X. Este módulo puede ser llamado desde los controladores o a nivel de servicio de dominio. Lo importante es centralizar la lógica para que sea consistente en todos los puntos de entrada. Si la autorización falla, se retorna 403 Forbidden. Definiremos claramente en la documentación qué acciones corresponden a qué nivel de rol.

* **Implementación de eventos internos:** En el backend codificaremos la capacidad de publicar eventos de dominio. Por ejemplo, podríamos crear una clase `DomainEvents` que tenga un método `publish(evento: DomainEvent)` y maneje la entrega. En una implementación básica, puede mantener una lista de listeners en memoria. Si usamos librerías, Ktor en sí no provee un bus, pero existen bibliotecas de event bus para Kotlin, o podemos implementar fácilmente un patrón Observer. Cada vez que en la lógica de negocio se produzca un evento, llamamos a `DomainEvents.publish(EventType.X(data))`. Los suscriptores registrados (posiblemente registrados durante la inicialización de la app) recibirán la notificación. Para habilitar la opción de Kafka u otro, `DomainEvents` podría tener dos implementaciones intercambiables: una para local (in-memory) y otra que envíe a Kafka. Esto de nuevo seguiría el patrón de puerto/adapter: un puerto `EventoPublisher` con adaptador `EventoPublisherKafka`, etc., inyectado según config.

  Si la organización opta por una arquitectura más microservicios, podríamos extraer ciertos manejadores en otro servicio, los cuales se suscriben a Kafka. Pero eso es futuro; inicialmente todo puede residir en el mismo proceso para menor complejidad.

* **Logs y auditoría en backend:** Usaremos un *logger* (posiblemente Kotlin Logging o slf4j) para registrar eventos importantes. Incluir contextos (como ID de petición, usuario) para trazabilidad. De ser posible, integrar **MDCDiagnostic** to tag logs per request, making debugging easier in distributed scenarios. Log level configurable (info, debug). Los eventos de auditoría (como "usuario X subió artefacto Y") se loguearán a nivel info o warn con un formato fácilmente filtrable.

* **Configuración y parámetros:** El backend será altamente configurable vía archivos de configuración externos o variables de entorno (ideal para 12-factor apps). Por ejemplo: config para DB (URL, user, pass), config para S3 (bucket, region, creds), puerto del servidor HTTP, opciones de TLS, tamaño máximo de artefactos a aceptar, etc. Se usará una librería de manejo de configs (como HOCON or YAML with kotlinx.serialization) y se documentará claramente cada parámetro. En Kubernetes, estas configs se mapearán a ConfigMaps/Secrets.

* **Entrega del backend:** Se producirá un **contenedor Docker** minimalista con el binario Kotlin/Native o la fat-jar (si se usa JVM) junto con un JRE delgado. Si es nativo, no necesita JRE. La imagen estará basada en Alpine o Distroless para minimizar tamaño y superficie de ataque. Esto facilita que la aplicación se despliegue en cualquier entorno containerizado.

En resumen, el backend combinará la potencia de Kotlin (tipo seguro, conciso) con un stack no bloqueante para lograr alto rendimiento. Aseguraremos que la implementación sigue los lineamientos de arquitectura definidos, manteniendo separación de concerns y permitiendo sustituir componentes (p. ej., almacenamiento, mensajería) con mínima fricción. El resultado será un servidor capaz de manejar el núcleo del repositorio con fiabilidad y velocidad.

## Especificaciones técnicas del Frontend

El frontend será una aplicación web **SPA (Single Page Application)** desarrollada con **Angular 19+**, aprovechando las últimas características y mejoras de este framework. A continuación se detallan consideraciones técnicas y de diseño para la interfaz de usuario:

* **Stack y lenguaje:** Angular utiliza **TypeScript** como lenguaje principal, lo que aporta tipado estático y mejor mantenimiento en un proyecto grande. Se usará Angular v19 o superior, asegurando compatibilidad con ECMAScript moderno y los *best practices* más recientes. Dado que Angular 15+ introdujo módulos independientes (*standalone components*), veremos si Angular 19 mantiene dicha filosofía; en cualquier caso estructuraremos el frontend en módulos lógicos (Feature Modules) para cargar funcionalidad bajo demanda (*lazy loading*), mejorando el rendimiento inicial de carga.

* **Comunicación con backend:** La aplicación consumirá las **APIs REST** proporcionadas por el backend. Para garantizar consistencia y facilitar el desarrollo, se generará un **cliente API** a partir de la especificación OpenAPI. Podemos usar herramientas como `openapi-generator` para TypeScript-Angular, que producirán servicios y modelos en TypeScript con las definiciones exactas de los endpoints. Esto evita errores manuales y asegura que si la API cambia, podamos regenerar el cliente fácilmente. Los servicios Angular encapsularán las llamadas HTTP usando `HttpClient`, manejando tokens de autenticación (incluyendo el JWT en los headers de cada petición automáticamente, quizás mediante un *HttpInterceptor* global).

* **Arquitectura de la aplicación Angular:** Se seguirá una estructura modular:

    * Un módulo principal (AppModule) con componentes globales (barra de navegación, layout básico).
    * Módulos funcionales para cada sección: por ejemplo, `RepositoryModule` para la exploración de repositorios y artefactos, `AdminModule` para administración de usuarios y repos, `AuthModule` para pantallas de login/registro, etc. Cada módulo cargará rutas hijas configuradas en el router Angular.
    * Uso del **Angular Router** para manejar múltiples vistas: rutas como `/login`, `/repos/<repoId>` (lista artefactos), `/repos/<repoId>/artifact/<artifactId>` (detalle de artefacto), `/admin/users`, etc.
    * Implementación de **guardas de ruta** (Route Guards) para proteger secciones que requieran autenticación o ciertos roles. Por ejemplo, el guard verificará si hay un JWT válido en almacenamiento (o en un servicio AuthState) y posiblemente consulta al backend para validar sesión o roles antes de permitir acceso a rutas admin.

* **Diseño visual y componentes:** Para acelerar el desarrollo y lograr un estilo consistente, se empleará un set de componentes UI maduros, probablemente **Angular Material** (ya que Angular Material provee componentes ya estilizados y accesibles, como tablas, formularios, diálogos, toolbars, etc.). Esto nos da un **look & feel** profesional sin diseñar todo de cero. Podríamos usar un tema predeterminado adaptado a la identidad de la organización. Aspectos de UI a destacar:

    * **Listados y tablas:** Mostrar listas de artefactos/repositorios, posiblemente paginadas si hay muchos elementos. Angular Material Table o list con *infinite scroll* podrían usarse.
    * **Formularios:** Para login, registro de usuarios, creación de repos, usaremos `<form>` reactivos (Reactive Forms de Angular) con validaciones tanto en frontend (campos requeridos, formato) como mostrando mensajes de error amigables. Por ejemplo, verificar que un nuevo ID de repositorio solo contenga caracteres permitidos antes de enviar.
    * **Feedback y notificaciones:** Incorporar *snack-bars* o alertas para informar al usuario de resultados de acciones (ej. "Artefacto subido con éxito", "Error: permiso denegado"). Utilizar colores/indicadores claros (verde éxito, rojo error).
    * **Navegación:** Un menú lateral o superior para cambiar entre secciones (Repos, Admin, Perfil, etc.). Debe resaltar la sección activa y permitir esconderlo en pantallas pequeñas. Soportar i18n si se requiere (inicialmente probablemente solo español, pero podríamos dejar el soporte preparado).

* **Gestión de estado y rendimiento:** Para datos que se comparten entre componentes (por ejemplo, la lista de repos que puede necesitarse en varias vistas, o la info del usuario autenticado), valoraremos usar una solución de gestión de estado centralizado como **NgRx (Redux)** o servicios singleton con BehaviorSubjects (RxJS) para un patrón Observable. Dado el tamaño moderado del proyecto, se podría iniciar con servicios simples que cacheen ciertos datos en memoria (ej: un `AuthService` que mantiene el usuario logueado, un `RepoService` que almacena en memoria la lista de repositorios ya obtenida para evitar recargar constantemente). Si la complejidad crece, NgRx sería una buena opción para manejar estado global de forma más estructurada.

  También implementaremos *lazy loading* para módulos de rutas no críticas: por ejemplo, el módulo de administración puede cargarse solo cuando un admin navega allí, reduciendo el tamaño inicial de bundle para un desarrollador típico que solo usa la parte de repositorios. Angular CLI con producción build optimizará el código (tree-shaking, minificación), esperamos entregar un bundle eficiente.

* **Seguridad en el frontend:**

    * Proteger el almacenamiento de tokens JWT: preferiblemente, almacenarlos en memoria (variable de un servicio) durante la sesión, o en `sessionStorage` en lugar de `localStorage` para mitigar persistencia excesiva. Evitar exponer el token a JavaScript de terceros (no usar JSONP, etc.). Considerar usar la **Auth0 JWT best practices** en SPAs (usar cookie HttpOnly con token de acceso corto + refresh token seguro, etc.) dependiendo del nivel de seguridad necesario.
    * Angular ayuda a prevenir **XSS** escapando contenido en plantillas por defecto. Ser cuidadosos si usamos `innerHTML` o bypasses (incluir sanitización de cualquier contenido dinámico que se inserte como HTML, aunque en este tipo de app es raro necesitar).
    * **CSRF**: Si optamos por JWT no hay estado de sesión en server, por lo que CSRF no aplica igual; si hubiéramos usado cookies de sesión, tendríamos que incluir tokens CSRF en solicitudes modifratorias. Aun así, las APIs rechazando peticiones sin un JWT válido nos protegen de llamadas forjadas desde otros sitios.
    * Controlar errores globales: implementar un `HttpInterceptor` que capture respuestas 401/403 para, por ejemplo, redirigir a login o mostrar mensaje "No autorizado". También manejar expiración de token (si backend devuelve 401 token expirado, podemos refrescar si hay refresh token o forzar login de nuevo).

* **Testing del frontend:** El proyecto Angular incluirá pruebas unitarias para componentes y servicios usando **Karma + Jasmine** (framework por defecto) o posiblemente **Jest** si se configura. Se escribirán pruebas para asegurar que componentes clave renderizan datos correctamente dada cierta entrada (por ejemplo, que el componente de lista de artefactos muestra un artefacto publicado en la lista). También se configurarán **pruebas end-to-end (E2E)**, probablemente con **Cypress** o **Protractor** (si aún está soportado en Angular 19, aunque Protractor está deprecado). Las pruebas E2E permitirán simular flujos completos en un entorno simulado con el backend (posiblemente usando una instancia de backend de prueba o simulando respuestas con interceptores), para garantizar que la integración funciona (por ejemplo, probar que un usuario con permisos limitados no ve opciones de admin en la UI, etc.). Esto se amplía en la sección de estrategia de testing.

* **Documentación y mantenimiento:** Para la parte de frontend, se generará documentación de componentes y servicios utilizando herramientas como **Compodoc** (genera documentación de Angular a partir de los comentarios y la estructura del código). Así, los desarrolladores de frontend (presentes y futuros) pueden entender rápidamente la arquitectura de la app, los módulos y dependencias. También podrían usarse Storybook u otra herramienta para documentar componentes visuales, aunque dado que esta aplicación es más específica (no es una librería UI general) quizás con compodoc sea suficiente. Mantendremos también un estilo de código consistente usando linters (TSLint o ESLint con reglas para Angular).

* **Deploy del frontend:** La aplicación Angular, una vez compilada para producción, genera archivos estáticos (HTML, JS, CSS). Estos archivos se pueden servir de dos maneras:

    1. Incluirlos en el mismo contenedor del backend (por ejemplo, poniendo el build en resources estáticos que Ktor sirva en `/`). Esto facilita que haya un solo servicio a desplegar, sirviendo tanto la API como la UI.
    2. O desplegarla por separado, por ejemplo, en un bucket S3 con CloudFront (si fuera público) o en un Nginx container. Dado que la UI probablemente requiera autenticación, quizás lo más simple es servirla desde el mismo servidor web del backend restringido.

  En cualquier caso, necesitaremos configurar el **routing** del lado servidor para que las rutas del frontend (no estáticas) redirijan al `index.html` (modelo SPA) y deje que Angular maneje la navegación. Por ejemplo, una petición a `/admin/users` no debe dar 404 en el servidor, sino devolver la página principal y Angular routing mostrará la vista adecuada. Esto se consigue en Ktor configurando un fallback para rutas no API.

En resumen, el frontend Angular se construirá para ser una herramienta potente y sencilla de usar para los distintos usuarios del sistema, alineada con los principios de SPA moderna: reactiva, modular, segura y bien integrada con el backend. Su desarrollo aprovechará las capacidades de Angular para crear una experiencia fluida y consistente, mientras que a nivel de código se mantendrá organizada y testeada para asegurar su calidad.

## Estrategia de testing

Garantizar la calidad del sistema es fundamental dada su criticidad en el proceso de desarrollo de software. La estrategia de testing abarcará múltiples niveles, desde pruebas unitarias de funciones individuales hasta pruebas de integración de todo el sistema desplegado. Además, se promoverá una cultura de **TDD (Desarrollo guiado por pruebas)** durante la implementación para prevenir defectos desde el inicio. A continuación se detallan los componentes de la estrategia de pruebas:

* **Pruebas unitarias (backend):** Cada componente de la lógica de dominio y utilitarios del backend tendrá pruebas unitarias asociadas. Gracias a la arquitectura hexagonal, podremos usar **dobles de prueba** (mocks/stubs) para los puertos de infraestructura. Por ejemplo, probaremos el caso de uso *PublicarArtefacto* en aislamiento, simulando un almacenamiento (para no realmente escribir archivos) y verificando que sigue la lógica correcta (rechaza si ya existe versión, guarda metadata correctamente si no, publica evento). Utilizaremos frameworks de test como **JUnit5** con Kotlin (o Spek/Kotest para un estilo más DSL), y librerías de mocking como **MockK** para crear stubs de interfaces. El objetivo es cubrir las rutas de código críticas, incluyendo validaciones de inputs, reglas de negocio (p.ej. que un SNAPSHOT puede sobreescribirse pero una release no, etc.), comportamientos del modelo RBAC, etc. Los criterios de aceptación de cada funcionalidad se traducirán inicialmente en tests (siguiendo TDD), asegurando que la implementación satisface los requerimientos. Apuntaremos a un alto nivel de cobertura de código (idealmente > 80% en módulo de dominio y utils), aunque más importante que la métrica, nos enfocaremos en cubrir casos representativos, bordes (ej: nombre de artefacto inválido, intento de duplicado) y condiciones de error.

* **Pruebas unitarias (frontend):** De forma similar, el frontend Angular incluirá **pruebas unitarias para componentes y servicios**. Usando el framework de test de Angular (Karma + Jasmine por defecto), verificaremos que los componentes:

    * Renderizan la vista correcta dadas ciertas entradas (por ejemplo, pasar un objeto Artefacto simulado al componente de detalle y comprobar que muestra nombre, versión, etc.).
    * Ejecutan la lógica correcta en respuesta a eventos de usuario (por ejemplo, al hacer click en "Eliminar artefacto", que llame al servicio correspondiente y muestre confirmación).
    * Los servicios de Angular tendrán pruebas donde se mockea el HttpClient (por ejemplo con HttpTestingController) para simular respuestas del backend y verificar que manejamos bien los datos (ej: que el AuthService al recibir credenciales devuelve un token y guarda estado de usuario).

  Angular Material provee utilidades para test, y vigilaremos que los tests no se acoplen a detalles internos (p.ej., preferir buscar texto en lugar de clases CSS que podrían cambiar). Asimismo, configuraremos estos tests para que corran en CI, asegurando que no fallen de forma intermitente.

* **Pruebas de integración (backend - Testcontainers):** Para garantizar que los distintos módulos del backend funcionan en conjunto con sus dependencias reales, usaremos **Testcontainers**. Este framework nos permite lanzar contenedores Docker desde código de test, para servicios como bases de datos o simulaciones de S3, y luego ejecutar pruebas contra el sistema apuntando a esos contenedores. Algunos escenarios de integración:

    * Levantar un contenedor de **PostgreSQL** en puerto aleatorio, configurar la app (o parte de la app) para usar esa DB, preparar esquemas/migraciones, y probar por ejemplo que la creación de usuarios realmente persiste y se puede recuperar. Esto valida la capa de acceso a datos real.
    * Usar **LocalStack** (un contenedor que imita AWS) para simular **S3**: en una prueba, podríamos inicializar LocalStack S3, configurar el adaptador S3 del sistema para que use ese endpoint, y luego llamar a la lógica de guardar artefacto, comprobando que el objeto aparece en el bucket simulado y que podemos leerlo de vuelta. Esto asegura que la integración con AWS SDK funciona como esperado.
    * Probar la publicación y consumo de eventos: si implementamos un adaptador Kafka para eventos, con Testcontainers podemos levantar un **Kafka** temporal y verificar que al emitir ciertos eventos se escriben en el topic, o incluso lanzar un consumidor simulado para ver la difusión.

  Otra forma de pruebas integrales es lanzar *la aplicación completa* en un entorno controlado y hacerle peticiones. Esto se acerca a pruebas end-to-end del backend: por ejemplo, con test frameworks como RestAssured o directamente usando HttpClient, podríamos:

    * Arrancar la aplicación (ya sea en modo JVM normal, o incluso dentro de un container Docker usando Testcontainers, aunque esto último es más pesado).
    * Ejecutar un flujo completo: crear usuario, login, subir un artefacto a un repo, listar artefactos, descargarlo, etc., verificando cada respuesta. Esto simula cómo un cliente real (o la UI) interactúa, asegurándonos que la configuración global (routing, filtros, etc.) está bien.

  Todas estas pruebas de integración se pueden etiquetar para ejecutarse en CI en etapas posteriores (ya que son más lentas que unitarias). Quizá configuraremos para que se ejecuten en paralelo en contenedores GitLab CI or GitHub Actions.

* **Pruebas end-to-end (E2E) del sistema completo:** Más allá del backend aislado, realizaremos pruebas E2E que incluyan el frontend. Utilizando herramientas como **Cypress** podemos automatizar un navegador que:

    1. Abra la aplicación web (desplegada en una URL local de test o en un contenedor).
    2. Interactúe con la UI tal como lo haría un usuario: llenar campos de login, click en botones de subir archivo (podemos simular seleccionar un archivo local para upload), navegar a pantallas, etc.
    3. Verifique que el resultado en la interfaz es el esperado (por ejemplo, tras subir un artefacto, que aparezca en la lista; o que al intentar acceder a admin sin permisos, muestre error).

  Estas pruebas E2E requieren que tanto el frontend como backend estén levantados. Podemos lograrlo en CI usando Docker Compose (arrancando la base de datos, backend y frontend contenedorizados) y luego ejecutando Cypress contra ese entorno. Alternativamente, en desarrollo se puede correr el backend en local, `ng serve` para frontend, y Cypress apunta a `localhost`. Estas pruebas dan confianza de que **todo el sistema** funciona cohesivamente y las integraciones cliente-servidor son correctas (contratos API respetados, CORS configurado bien, etc.).

  También consideraremos E2E para casos de error: intentar operaciones prohibidas y esperar mensaje de error correcto, etc.

* **Pruebas de rendimiento:** Dado que uno de los objetivos es alto rendimiento, incorporaremos pruebas de carga una vez el sistema esté funcional. Usando herramientas como **JMeter, Gatling (Scala) o k6 (JavaScript)**, simularemos múltiples clientes descargando y subiendo artefactos para medir throughput y detectar cuellos de botella. Por ejemplo, un escenario: 100 usuarios concurrentes realizando descargas de un artefacto de 50MB, medir tiempo medio y uso de CPU/RAM. Otro: ráfaga de publicaciones simultáneas para ver cómo maneja la base de datos y el almacenamiento. Estas pruebas nos guiarán para tunear parámetros (hilos de IO, pool de DB, tamaño de buffers, etc.). No es habitual incluir tests de carga en la CI de cada commit por su duración, pero se pueden ejecutar en hitos (antes de release) o en entornos dedicados. Documentaremos los resultados y ajustaremos la arquitectura si algún objetivo de performance no se cumple.

* **TDD y pipeline de calidad:** Desde el inicio se fomentará escribir la prueba antes o a la par de la funcionalidad (especialmente en core de negocio). El pipeline de integración continua (CI) ejecutará:

    * Linter/format (asegurando estilo consistente).
    * Compilación.
    * Pruebas unitarias backend y frontend.
    * Pruebas de integración (posiblemente en un stage separado).
    * Build de contenedores y despliegue a un entorno de staging donde podrían correr E2E.

  Solo si todos los pasos pasan, los cambios se considerarán listos para merge. Adicionalmente, usaremos análisis estático (herramientas como SonarQube) para atrapar *code smells*, problemas de complejidad o potenciales bugs. Esto forma parte de la calidad general.

* **Testcontainers en desarrollo local:** Vale resaltar que el uso de Testcontainers no solo es para CI, sino que también facilita a desarrolladores correr pruebas integrales en sus máquinas sin instalar nada más que Docker. Esto reduce el "works on my machine" syndrome, ya que el entorno de pruebas es reproducible.

Con esta estrategia multifacética, esperamos lograr una **cobertura de pruebas completa**: unidad para lógica interna, integración para contratos entre módulos, y E2E para la experiencia real. Esto proporcionará confianza en cada release, evitando regresiones al refactorizar o extender el sistema, y asegurando que el PRD se cumple en la implementación real.

## Estrategia de documentación

Una documentación clara y actualizada será un entregable crítico junto con el software. Dado que el sistema será utilizado y mantenido por desarrolladores, es vital proveer información tanto para **usuarios finales** (ej. desarrolladores que consumirán el repositorio) como para **colaboradores del proyecto** (desarrolladores que trabajen en el código). La estrategia de documentación abarcará:

* **Documentación de la API (OpenAPI/Swagger):** El backend expondrá un documento OpenAPI (versión 3) describiendo todos los endpoints, métodos, parámetros, esquemas de datos, códigos de respuesta, etc. Utilizaremos una librería en Kotlin que genere esta especificación automáticamente a partir del código (por ejemplo, usando anotaciones o un plugin de Ktor). Esta documentación estará disponible de forma dinámica a través de una interfaz Swagger UI alojada en el propio servicio (p.ej. en la ruta `/swagger-ui` o similar), para que cualquier usuario pueda explorar y probar los endpoints en vivo (con la salvedad de necesitar autenticación para la mayoría). Adicionalmente, se podrá publicar el archivo OpenAPI (JSON/YAML) en el repositorio de código para que otros servicios o SDKs puedan usarlo (incluso integrarlo en Postman collections, etc.). Mantener la API documentada no solo ayuda a los clientes del sistema sino que también fuerza buenas prácticas en el diseño (nombres claros, evitar incoherencias).

* **Manual de usuario / guía de uso:** Se preparará una **guía de usuario** destinada a desarrolladores y DevOps que van a interactuar con el repositorio. Esta guía, probablemente en formato Markdown o similar (posible publicación en un portal Wiki interno o en un sitio estático), incluirá:

    * **Introducción para usuarios:** qué es el sistema, qué tipos de artefactos soporta.
    * **Cómo configurar herramientas cliente:** por ejemplo, "¿Cómo configuro Maven para usar este repositorio?" (incluir un snippet de `settings.xml` con la `<mirror>` apropiada apuntando a nuestro URL, y cómo añadir credenciales). O "¿Cómo publico un paquete npm?" (ejemplo de `.npmrc` con registry y auth token, y uso de `npm publish`).
    * **Flujos comunes:** explicar cómo subir una nueva librería interna paso a paso, cómo solicitar acceso a un repo, cómo promover una versión snapshot a release, etc., según las políticas que definamos.
    * **Capturas de pantalla** de la UI (si es suficientemente estable) mostrando cómo navegar y realizar tareas (crear repos, buscar artefactos, etc.).
    * **Solución de problemas comunes:** e.g., "Error 401 al hacer npm install - verificar que tu token no expiró", u "Obtengo 409 Conflict al subir un artefacto - eso significa que esa versión ya existe", dando orientaciones.

  Esta documentación de usuario debe escribirse en un lenguaje accesible, asumiendo conocimiento técnico básico de quienes la lean (programadores, DevOps). Podría versionarse junto al código para actualizarla con cada release.

* **Documentación técnica interna (para desarrolladores del proyecto):** Aquí nos enfocamos en describir la arquitectura y decisiones para cualquier nuevo miembro del equipo o colaborador:

    * **Descripción de arquitectura:** un documento (como este PRD, refinado tras implementación) que incluya diagramas de alto nivel de los componentes hexagonales, el flujo de eventos, la distribución en despliegue (ej. diagrama de Kubernetes mostrando pods: frontend, backend, DB, etc.). Se pueden emplear diagramas UML o de arquitectura lógica, creados con herramientas como PlantUML, diagram.as o similares, y guardados en el repositorio para mantenerlos actualizados.
    * **Esquema de datos:** documentación de las principales tablas de la base de datos (campos, índices) y cómo se relacionan. Esto ayuda para mantenimiento, migraciones futuras o integraciones (ej. si alguien necesita hacer una consulta directa).
    * **Detalles de módulos:** una breve descripción de cada módulo de código (por ejemplo, module "domain", module "infrastructure", module "application" si se separa así) y qué contiene. Así como detalles de tecnologías usadas (ej. "Usamos Exposed para ORM, aquí un ejemplo de cómo hacer consultas", "el adaptador S3 está en X clase, usando la lib Y").
    * **Guía de desarrollo:** pasos para correr el proyecto localmente, ejecutar los tests, construir los contenedores. Esto podría ser el README del repositorio de código, orientado a futuros desarrolladores. Incluir comandos comunes de gradle/maven para backend, npm para frontend, etc.
    * **Decisiones de arquitectura (ADR):** en caso de decisiones complejas, registrar ADRs (Architecture Decision Records) breves, indicando qué alternativas se evaluaron y por qué se eligió la solución final (por ejemplo, "¿Por qué Kotlin/Native en lugar de Node.js o Java?", "¿Por qué Ktor vs Spring Boot?", "¿Por qué JWT para auth?", etc.). Esto previene re-discutir lo mismo más adelante y deja constancia para onboarding.

* **Automatización de la documentación:** Parte de la documentación se generará automáticamente:

    * El mencionado OpenAPI se genera del código.
    * Podemos usar **Dokka** (herramienta de documentación para Kotlin) para generar un sitio HTML con la referencia de clases, funciones y comentarios KDoc del backend. Si los desarrolladores documentan las funciones y clases, Dokka produce una salida similar a Javadoc, útil para entender la API interna.
    * Para el frontend, **Compodoc** generará una documentación estática con información de componentes, servicios, módulos, incluyendo gráficos de dependencia. Esto puede integrarse en la entrega (por ejemplo, servirlo en GitHub Pages o en un minisitio interno).
    * Incluir generación de diagramas a partir del modelo (opcionalmente, por ejemplo, si definimos entidades en la base, usar una herramienta para generar un ERD automáticamente).

  Se integrará la generación de documentación en el pipeline de CI/CD en la medida de lo posible, para que con cada versión release se actualice la documentación publicada. Por ejemplo, podríamos tener un job que, tras pasar tests, genere el sitio de documentación técnica y lo despliegue (si es interno, en un artefacto accesible o en un servidor web). Al menos, se hará manualmente en cada release importante.

* **Swagger UI y sandbox:** Para facilitar a los integradores probar la API, habilitaremos **Swagger UI** con interacción: esto permite, por ejemplo, que un desarrollador de otro equipo abra la página de API, elija un endpoint de publicación, adjunte un archivo y pruebe una request in-situ (tras autenticarse dentro de Swagger UI). Esto sirve como documentación viva. Aseguraremos que la UI de Swagger no esté abierta al mundo si contiene operaciones sensibles; podría requerir login de admin para acceder, o estar en una red interna segura.

* **Comentarios en código y mejores prácticas:** Más que un documento por separado, nos aseguraremos de mantener un código auto-documentado. Nombres significativos, funciones pequeñas, y comentarios donde el propósito no sea obvio. Especialmente en el dominio, cada caso de uso o regla de negocio tendrá su javadoc/kdoc explicando qué hace. Esto, sumado a la documentación generada, hace que si alguien lee el código pueda seguirlo sin depender únicamente de documentación externa (que a veces se desvincula del código real con el tiempo).

* **Documentación para despliegue:** Incluiremos instrucciones para desplegar el sistema en Kubernetes: por ejemplo, archivos Helm Chart o YAMLs con explicaciones de cada valor (documentados quizás en un README en la carpeta de despliegue). Esto abarca la configuración de variables de entorno, cómo conectar con DB/S3, etc. Esta guía de despliegue asegurará que equipos de infraestructura puedan instalar el sistema sin ambigüedades.

* **Mantenimiento de la documentación:** La documentación no debe ser estática; estableceremos la política de actualizar la documentación al introducir cambios relevantes. Idealmente, en cada historia de usuario completada, si afecta el comportamiento de usuario o API, se actualizará la sección correspondiente de la guía de usuario o la referencia API. Revisiones de código también verificarán si la documentación requiere cambios. Para cambios arquitectónicos, actualizar diagramas y descripción en el documento técnico. Podemos planificar **revisiones periódicas** de documentación (ej. antes de una versión mayor, hacer un repaso de la doc a ver si algo quedó obsoleto).

En conjunto, nuestra estrategia garantizará que toda persona que interactúe con el sistema –ya sea para usarlo o para modificarlo– cuente con información confiable, actual y fácilmente accesible. Esto reduce curva de aprendizaje, previene malentendidos y en última instancia mejora la adopción y el mantenimiento a largo plazo del producto.

## Consideraciones de seguridad y escalabilidad

Esta sección resume aspectos críticos relativos a seguridad de la solución y su capacidad de escalar, complementando lo mencionado en requisitos y diseño:

### Seguridad

* **Modelo de confianza cero (Zero Trust):** El sistema asumirá que ninguna petición es confiable hasta que se demuestre lo contrario. Esto significa exigir autenticación en todas las llamadas (salvo quizás lectura anónima en repos públicos si habilitado explícitamente) y validar todos los inputs. Cualquier dato proveniente de clientes (parámetros de URL, cuerpos JSON, formularios) será validado/racionalizado para evitar inyecciones o comportamiento inesperado.

* **Seguridad de artefactos:** Aunque el repositorio almacena binarios sin examinar su contenido, debemos asegurar su integridad y evitar manipulaciones:

    * Se calcularán **hashes criptográficos (SHA-256, por ejemplo)** de cada archivo al almacenarlo, guardándolos como parte de los metadatos. El cliente (Maven, npm) ya comprueba a veces SHA1, pero podemos reforzar con SHA-256. Esto detecta corrupción o alteraciones. Si un artefacto se corrompe en almacenamiento, se podría detectar al compararlo con su hash registrado.
    * Si se desea mayor garantía, podríamos integrar **firmas digitales**: por ejemplo, permitir almacenar firmas PGP asociadas a JARs (como hace Maven Central) y ofrecer esas .asc a consumidores. Esto no estará inicialmente, pero la arquitectura podría soportarlo (como un campo extra o un artefacto adjunto).
    * **Inmutabilidad de artefactos:** una vez publicado un artefacto de versión release, el sistema puede bloquear cualquier modificación o sobreescritura. Esto evita ataques de *repudiation* o la conocida práctica maliciosa de reemplazar un artefacto por otro con el mismo nombre. Para snapshots, que son mutables por naturaleza, se puede permitir sobreescritura pero quizás limitando a ciertos roles y guardando un registro de versiones.
    * **Escaneo de seguridad (futuro):** Aunque no inmediato, tener eventos de `ArtefactoPublicado` permite enganchar un proceso de escaneo de vulnerabilidades (usando herramientas SCA) para analizar dependencias o código de los artefactos publicados. Esto sería una capa de defensa para evitar que librerías con malware entren en la cadena de suministro interna. En la documentación se mencionará como posibilidad a futuro.

* **Protecciones en producción:**

    * **TLS/HTTPS:** Se recomienda desplegar siempre detrás de un proxy TLS (en Kubernetes, un Ingress con certificado) o configurar TLS en la aplicación. Toda comunicación de login y transferencia de artefactos debe estar cifrada para evitar espionaje de credenciales o código propietario.
    * **Headers de seguridad:** El servidor deberá enviar cabeceras HTTP apropiadas, por ejemplo `Content-Security-Policy` (para la app Angular, restringiendo orígenes de scripts), `X-Content-Type-Options: nosniff` (para evitar interpretaciones erróneas de tipo), `X-Frame-Options: DENY` (UI no embebible en iframe de terceros), etc. Estas cabeceras endurecen la aplicación contra ciertas clases de ataque (clickjacking, etc.).
    * **Rate Limiting / DoS:** Dado que este es un servicio interno, la amenaza de DoS es menor, pero no imposible (un bug en un script CI podría saturar el repo con peticiones). Podríamos implementar un **rate-limiter** básico en el gateway o en la app (p.ej. no más de X publicaciones por minuto por usuario, o limitar tamaño de artefacto a cierto máximo configurable). En Kubernetes, además, se puede configurar HPA para escalar si la carga sube, mitigando un poco DoS volumétrico aumentando capacidad (aunque con límites).

* **Seguridad de la cadena CI/CD:** Como el desarrollo sigue TDD y genera contenedores, nos aseguraremos de:

    * Usar **imágenes base seguras** (actualizadas, minimales).
    * Escanear imágenes de Docker por vulnerabilidades (integrar algo como Trivy o clair en el pipeline).
    * Mantener dependencias actualizadas: habilitar alerta de dependencias (DepShield, Renovate bot) para actualizar librerías de Kotlin, Angular y otros ante parches de seguridad.
    * Proteger los secretos (como claves de S3, contraseñas DB) usando K8s Secrets y no exponiéndolos en logs ni en código.

* **Logs y monitoreo de seguridad:** Los logs de auditoría mencionados facilitarán detectar comportamientos anómalos (p.ej. un usuario descargando cientos de artefactos de golpe, quizá indica credenciales comprometidas). Se puede configurar alertas en el sistema de monitoreo (Prometheus/Alertmanager) por ciertos patrones, y a futuro integrar con SIEM corporativo si existe.

* **Backups y recuperación:** Aunque no exactamente seguridad, es importante tener **backups regulares** de la base de datos (que contiene usuarios, metadatos, etc.) y, si se usa almacenamiento local, también de los artefactos en disco (si S3 se usa, eso ya es redundante con 99.999999999% durabilidad, pero en local se debe planificar un backup a otro storage). Esto para resiliencia ante pérdidas de datos, ransomware, etc. Incluiremos documentación sobre cómo realizar backups (scripts, etc.) y cómo restaurar en caso de desastre.

### Escalabilidad

* **Clustering y replicación:** El diseño asume que múltiples instancias del backend pueden correr en paralelo. Para que esto funcione sin conflictos:

    * Las instancias deben ser **sin estado** en cuanto a sesión (usamos JWT, así que bien) y no guardar nada local que no esté replicado. Si se usa almacenamiento local de artefactos, se tendría que usar un volumen compartido entre pods o, mejor, cada instancia sólo sirviendo su propio volumen y usando un **proxy nivel superior** para dirigir peticiones al nodo que las tiene. Esto último es complejo; por eso recomendamos S3 para escenarios multi-nodo, eliminando la necesidad de coordinar almacenamiento.
    * **Coordinación de caché:** Si implementamos algún caché en memoria (ej. de metadatos), en multi-nodo podríamos tener inconsistencia momentánea. Para la mayoría de cosas esto no es crítico (ej., si un nodo cacheó la lista de artefactos por 1 minuto, y otro nodo añade uno nuevo, puede que el primero tarde un minuto en mostrarlo). Podemos tolerar ligeras eventualidades o usar mecanismos de **cache busting** con eventos: por ejemplo, cuando un artefacto nuevo se publica, emitir un evento que todas las instancias escuchan y así invalidan su caché local. Este es un patrón común para mantener escalabilidad sin demasiado acoplamiento.
    * **Balanceo de carga:** En Kubernetes, normalmente un Service round-robin distribuirá peticiones. Debemos asegurarnos que operaciones de carga de archivos grandes funcionan bien en ese contexto (posibles timeouts de LB, etc., config de client\_max\_body\_size en ingress si se suben archivos > 1MB).
    * **Auto-escalado:** Definir métricas de autoescalado: CPU y memoria son típicas. Si la aplicación consume mucho CPU al servir artefactos (en criptografía, compresión, etc.), autoescalar por CPU tiene sentido. Otras métricas: número de peticiones por segundo (Pod autoscaler custom metric) si medimos en Prometheus. Probaremos horizontal scaling manual primero.

* **Escalabilidad de almacenamiento:**

    * Con S3, realmente la escalabilidad la maneja AWS en backend (soporta altísimo throughput si se siguen buenas prácticas, como prefijar objetos de manera distribuida, lo cual en nuestro caso vendrá dado por nombres de artefactos; S3 por lo general ya no tiene los límites de prefijos de antes, pero igual, estaremos bien).
    * Con base de datos: dimensionar Postgres para carga de meta. Afortunadamente, la cantidad de metadatos es mucho menor que los artefactos en sí. Pero se debe monitorear conexiones (usar pool de conexiones, tuneado de  size), índices en columnas consultadas frecuentemente (por ej. buscar artefactos por nombre, filtrar por user para auditoría, etc.). Si escala en tamaño de datos, particionar tablas o archivado de registros antiguos (snapshot muy viejos se podrían borrar metadata cuando se borran binarios).
    * Si se escala a muchísimos artefactos (millones), podría considerarse separar metadatos de artefactos en servicios distintos (microservicio catalogador vs almacenamiento puro); pero esto es para un horizonte muy amplio.

* **Pruebas de escalabilidad:** Como parte del roadmap, al agregar más tipos de repos o usuarios, se debe retestar la escalabilidad. Por ejemplo, agregar repos Docker implicaría artefactos mucho mayores (imágenes), cómo afecta eso. Siempre se debe planear capacidad antes de saturar: monitorear uso de disco, conteo de objetos en S3 (costos también), etc., y hacer *sharding* de repos si uno solo se vuelve enorme (por ej., en lugar de un repo con 100k artefactos, distribuir en varios repos más específicos).

* **Multi-region (futuro):** En algunas organizaciones se puede requerir replicación geográfica (ej., un cluster en Europa y otro en América sirviendo los mismos artefactos para bajar latencia local). Aunque inicialmente fuera de alcance, nuestro diseño con S3 facilita esto si usamos un bucket con replicación multi-región o CDN front. Un enfoque futuro podría ser usar un servicio global (como AWS CloudFront as CDN for S3, o JFrog Distribution equivalent). Por ahora, dejamos la puerta abierta a esas mejoras sabiendo que el core permanece consistente.

En resumen, el sistema está pensado para **crecer de forma segura**. Mediante buenas prácticas de seguridad, minimizamos riesgos de brechas o abuso; mediante un diseño escalable y la opción de aprovechar servicios cloud, nos aseguramos de poder atender a un creciente número de usuarios y artefactos sin rediseñar desde cero.

## Seguridad de la Cadena de Suministro: Integración de SBOM y Grafos Merkle

La seguridad de la cadena de suministro de software se ha convertido en una preocupación crítica en el desarrollo moderno. Para abordar esta necesidad, Hodei Packages incorporará capacidades avanzadas de gestión de Software Bill of Materials (SBOM) y verificación criptográfica basada en grafos Merkle. Estas características permitirán a las organizaciones verificar la integridad de los artefactos, rastrear dependencias y gestionar vulnerabilidades de manera efectiva.

### Visión General y Fundamentos

Integrar la gestión de SBOM y la verificación criptográfica mediante grafos Merkle en Hodei Packages mejorará significativamente la seguridad de la cadena de suministro. Estas características proporcionarán verificación de integridad de artefactos, trazabilidad de dependencias y capacidades de gestión de vulnerabilidades críticas para las prácticas modernas de desarrollo seguro de software.

### Componentes Clave y Arquitectura

* **Generación y Gestión de SBOM:** El sistema soportará la generación, almacenamiento y recuperación de SBOMs para los artefactos subidos, siguiendo estándares de la industria como CycloneDX y SPDX.

* **Verificación Criptográfica mediante Grafos Merkle:** Implementaremos un sistema de almacenamiento direccionable por contenido utilizando Grafos Acíclicos Dirigidos (DAG) de Merkle para habilitar el versionado a prueba de manipulaciones y la verificación criptográfica de artefactos.

* **Integración con la Arquitectura Principal:** Estas características se implementarán siguiendo nuestra arquitectura hexagonal existente, añadiendo nuevas entidades de dominio, puertos y adaptadores mientras se mantiene una clara separación de responsabilidades.

### Especificaciones Funcionales

#### Gestión de SBOM

* **Generación de SBOM:**
  * Soporte para generación automática de documentos SBOM durante la carga de artefactos
  * Extracción de información de dependencias de formatos comunes de paquetes (Maven POM, package.json de npm, etc.)
  * Permitir la carga manual de SBOM junto con los artefactos
  * Soporte para formatos CycloneDX y SPDX con extensibilidad para futuros estándares

* **Almacenamiento y Recuperación de SBOM:**
  * Almacenar SBOMs como documentos inmutables y versionados vinculados a sus respectivos artefactos
  * Proporcionar endpoints API para recuperar SBOMs en múltiples formatos (JSON, XML, etc.)
  * Permitir consultar artefactos por componentes/dependencias listados en sus SBOMs

* **Análisis de SBOM:**
  * Implementar interfaces para la integración de escaneo de vulnerabilidades
  * Soportar la visualización de grafos de dependencias a través de la UI
  * Permitir comparar SBOMs entre versiones de artefactos para seguimiento de cambios

#### Verificación mediante Grafos Merkle

* **Almacenamiento Direccionable por Contenido:**
  * Almacenar artefactos y metadatos en un sistema direccionable por contenido basado en hashes criptográficos
  * Representar artefactos y sus dependencias como nodos en un DAG de Merkle
  * Asegurar que cada objeto sea identificado de forma única por el hash de su contenido

* **Firma y Verificación:**
  * Soportar la firma criptográfica de hashes raíz de Merkle para demostrar autenticidad
  * Implementar endpoints de verificación para validar la integridad de artefactos
  * Proporcionar APIs de generación y verificación de pruebas para comprobación eficiente de integridad

* **Historial Inmutable:**
  * Mantener un historial a prueba de manipulaciones de todas las versiones de artefactos
  * Posibilitar la verificación criptográfica de toda la cadena de dependencias

### Implementación Técnica

#### Extensiones del Modelo de Dominio

* **Nuevas Entidades de Dominio:**
  ```
  domain/
  ├── model/
  │   ├── sbom/
  │   │   ├── SbomDocument.kt          # Entidad para documento SBOM
  │   │   ├── SbomComponent.kt         # Componente en un SBOM
  │   │   └── SbomFormat.kt            # Enum de formatos soportados
  │   └── merkle/
  │       ├── MerkleNode.kt            # Nodo en el grafo Merkle
  │       ├── MerkleGraph.kt           # Representación del grafo
  │       ├── ContentHash.kt           # Value object para hash de contenido
  │       └── Signature.kt             # Firma criptográfica
  ```

* **Nuevos Puertos (Interfaces):**
  ```
  domain/
  ├── repository/
  │   ├── SbomRepository.kt            # Almacenamiento y recuperación de SBOM
  │   └── ContentAddressableStorage.kt  # Almacenamiento del grafo Merkle
  ├── service/
  │   ├── SbomService.kt               # Generación y análisis de SBOM
  │   ├── MerkleGraphService.kt        # Construcción y recorrido de grafos
  │   └── CryptographicService.kt      # Firma y verificación
  ```

* **Nuevos Eventos de Dominio:**
  ```
  domain/events/
  ├── sbom/
  │   ├── SbomGenerated.kt
  │   └── SbomAnalyzed.kt
  └── merkle/
      ├── ArtifactVerified.kt
      └── SignatureCreated.kt
  ```

#### Capa de Servicios de Aplicación

* **Nuevos Servicios de Aplicación:**
  * `SbomApplicationService`: Orquesta la generación, almacenamiento y análisis de SBOM
  * `VerificationService`: Maneja la verificación criptográfica de artefactos
  * `MerkleGraphBuilder`: Construye y mantiene grafos Merkle

* **Integración con Servicios Existentes:**
  * Extender `ArtifactService` para iniciar la generación de SBOM durante la carga de artefactos
  * Modificar el proceso de publicación de artefactos para incluir la creación de nodos Merkle
  * Añadir pasos de verificación a los flujos de descarga de artefactos

#### Implementaciones de Infraestructura

* **Adaptadores de Almacenamiento:**
  * `FilesystemContentAddressableStorageAdapter`: Implementa almacenamiento direccionable por contenido en el sistema de archivos
  * `S3ContentAddressableStorageAdapter`: Implementación similar para S3

* **Integración con Servicios Externos:**
  * `CycloneDXGenerator`: Genera SBOMs en formato CycloneDX
  * `SPDXGenerator`: Genera SBOMs en formato SPDX
  * `VulnerabilityScanner`: Integra con escáneres externos (opcional)

#### Extensiones de la Capa API

* **Nuevos Endpoints REST:**
  ```
  POST   /api/artifacts/{id}/sbom           # Generar/subir SBOM para un artefacto
  GET    /api/artifacts/{id}/sbom           # Recuperar SBOM de un artefacto
  GET    /api/artifacts/{id}/verify         # Verificar integridad del artefacto
  POST   /api/artifacts/{id}/sign           # Firmar un artefacto (solo admin)
  GET    /api/artifacts/{id}/proof          # Obtener prueba de verificación
  ```

* **Integración con Clientes:**
  * Proporcionar librerías cliente o plugins para Maven, npm, etc., para verificar artefactos
  * Documentar el uso de la API para integraciones personalizadas

### Mejoras de la Interfaz de Usuario

* **Visualización de SBOM:**
  * Añadir visualización de grafo de dependencias en la vista de detalle de artefacto
  * Mostrar información de vulnerabilidades cuando esté disponible
  * Proporcionar opciones de descarga de SBOM

* **Estado de Verificación:**
  * Mostrar indicadores de estado de verificación para los artefactos
  * Mostrar información de firma e historial de verificación
  * Proporcionar exportación de certificado de verificación

### Consideraciones de Seguridad

* **Estándares Criptográficos:**
  * Utilizar algoritmos criptográficos modernos (SHA-256/SHA-3 para hashing, Ed25519 para firmas)
  * Seguir prácticas seguras de gestión de claves
  * Soporte para módulos de seguridad de hardware para firma (mejora futura opcional)

* **Rendimiento de Verificación:**
  * Implementar verificación eficiente de pruebas de Merkle
  * Considerar verificación parcial para árboles de dependencias grandes
  * Utilizar estrategias de caché para artefactos verificados frecuentemente

### Estrategia de Testing

* **Pruebas Unitarias:**
  * Probar el cálculo de hash y las operaciones del árbol Merkle
  * Verificar la generación y análisis de SBOM
  * Probar la creación y verificación de firmas
  * Se utilizará Kotest con estilo de especificación descriptivo, seguido de nuestro enfoque TDD

* **Pruebas de Integración:**
  * Pruebas end-to-end para el flujo de carga-firma-verificación
  * Pruebas de rendimiento para verificación de artefactos grandes
  * Probar interoperabilidad con formatos estándar de SBOM
  * Se aplicarán técnicas de mocking con MockK para aislar componentes

* **Pruebas de Seguridad:**
  * Intentar verificar artefactos modificados (debería fallar)
  * Probar verificación con firmas inválidas
  * Verificar protección contra ataques de colisión de hash

### Alineación con la Arquitectura Existente

Esta mejora preserva la arquitectura hexagonal al:

1. Mantener entidades de dominio y lógica de negocio en la capa de dominio
2. Definir puertos (interfaces) claros para servicios de infraestructura
3. Implementar adaptadores de infraestructura que se ajusten a estos puertos
4. Utilizar servicios de aplicación para orquestar flujos de trabajo
5. Aprovechar el diseño basado en eventos para la integración del sistema

Las funcionalidades de SBOM y grafos Merkle se integrarán perfectamente con el sistema actual de autenticación y autorización, respetando los permisos de repositorio y roles de usuario.

## Roadmap inicial

Si bien la presente documentación describe la **visión global** y los requerimientos del sistema, la implementación se realizará en fases iterativas. A continuación se bosqueja un roadmap inicial con las etapas previstas, sabiendo que podría ajustarse según prioridades de negocio y aprendizajes durante el desarrollo:

* **Versión 0.1 – MVP Interno (aprox. 2-3 meses):**

    * **Alcance:** Soporte básico de repositorio Maven y npm con almacenamiento local, autenticación simple y UI mínima.
    * Implementar la estructura básica del proyecto (hexagonal layers, entidades de dominio principales).
    * Soportar publicar y descargar artefactos Maven (por REST, usando credenciales básicas) en un único repositorio preconfigurado.
    * Soportar publicar/instalar paquetes npm en un registro preconfigurado.
    * Autenticación: gestión rudimentaria de usuarios (crear usuarios vía script o endpoint admin), login JWT, rol admin vs usuario simples.
    * Frontend: página de login, página listing muy simple de contenidos del repos (puede que sin todas las comodidades, pero al menos ver que algo se almacenó).
    * Documentación: OpenAPI draft, README con instrucciones para probar.
    * **Objetivo:** Tener un sistema funcional end-to-end en entorno de prueba, limitado pero demostrando el flujo completo. Validar la arquitectura escogida con un caso real y obtener feedback de performance inicial.

* **Versión 0.5 – Funcionalidades núcleo completas (aprox. +2 meses):**

    * **Alcance:** Completar todos los requerimientos funcionales principales.
    * RBAC completo: gestión de roles por repositorio, UI para administrar usuarios/roles.
    * Múltiples repositorios configurables: endpoints para crear/listar repos, UI para gestionarlos, posibilidad de separar snapshot/release.
    * Almacenamiento S3: permitir configurar repos para usar S3; probar en entorno con AWS (o LocalStack) la conmutación entre local/S3.
    * UI mejorada: vistas de detalle de artefacto, componente de upload manual, tablas paginadas, búsqueda básica (por nombre, filtro).
    * Operaciones de mantenimiento: borrar artefactos (con confirmación), limpiar snapshots antiguos automáticamente (puede ser un job programado dentro del app).
    * Integración CI: documentación y testing con Maven/Gradle/npm actual contra el repo (ej., publicar un artefacto desde Maven plugin, etc., ajustando cualquier incompatibilidad).
    * Añadir pruebas de integración y E2E para todas estas funcionalidades.
    * **Objetivo:** Este sería un **Release 1.0** candidato para uso interno real. Debería poder reemplazar en funcionalidad básica a un Nexus/Artifactory para Maven/npm dentro de un equipo piloto.

* **Versión 1.x – Mejoras de rendimiento, extensibilidad y seguridad de la cadena de suministro (aprox. +2-4 meses, iterativo):**

    * Optimizar rendimiento donde métricas hayan mostrado cuellos: p.ej. activar streaming real en descargas, tuning de thread pools, caching de metadata frecuentes.
    * Implementar caching/proxy de repos externos (por ejemplo, actuar como proxy de Maven Central/npmjs para que las dependencias externas se cacheen localmente). Esto fue mencionado como posible mejora comparando con Nexus. Esto implica nuevas configuraciones de repos tipo *proxy* y manejadores que al no encontrar artefacto local lo busquen en remoto.
    * **Fase 1 de integración de SBOM y Merkle (MVP):** Implementación básica de generación de SBOM para artefactos Maven y npm, almacenamiento direccionable por contenido y API de verificación simple. Incluirá:
      * Entidades de dominio básicas (`SbomDocument`, `MerkleNode`, `ContentHash`)
      * Puertos principales (`SbomRepository`, `ContentAddressableStorage`)
      * Adaptadores de infraestructura para almacenamiento de SBOM y grafos Merkle
      * Endpoints REST básicos para generar/recuperar SBOMs
      * Implementación de verificación de integridad de artefactos mediante hashes
    * Añadir soporte a al menos un nuevo formato de artefacto para probar extensibilidad: candidato popular sería **imágenes Docker (OCI)** o **packages NuGet**. Docker implicaría incorporar un adaptador para el protocolo de registro Docker (JWT auth support, manifest and blob endpoints). Quizá NuGet es más sencillo. Elegir según demanda.
    * Internacionalización de la UI si es relevante (por ejemplo inglés/español).
    * Mejoras de seguridad: integración con LDAP/OAuth if needed by organization, o implementación de 2FA para la UI admin.
    * Feedback de usuarios: por ejemplo, mejorar la usabilidad de la UI (buscador de artefactos más potente, categorías, etiquetas a artefactos, etc.) en función de lo que los usuarios piloto sugieran.
    * **Objetivo:** Refinar el producto para una adopción más amplia en la organización, cubriendo casos de uso adicionales, mejoras de seguridad de la cadena de suministro y asegurando que el rendimiento se mantiene bajo cargas crecientes.

* **Versión 2.0 – Escalabilidad y características avanzadas (a mediano plazo):**

    * Despliegue HA multi-nodo validado: pruebas en cluster Kubernetes con failover, quizá implementando *leader election* para tareas programadas (si las hay) o mecanismo de sincronización entre nodos via eventos/Kafka.
    * Soporte multi-formato completo: Maven, npm, Docker, NuGet, PyPI, etc., según las necesidades. Convertir el producto en una verdadera alternativa unificada para la gestión de artefactos de cualquier tipo.
    * Funcionalidades tipo **Enterprise**: replicación geográfica, mirror entre instancias (ej., para tener un entorno de staging/prod con repos sincronizados), respaldo automatizado de artefactos, limpieza de repos basada en políticas avanzadas (ej: conservar última versión de cada major).
    * UI Dashboard con métricas de uso, gráficos (descargas por día, almacenamiento usado por repo, etc.).
    * Posible creación de un **CLI client** (tipo `repoctl`) para admins para facilitar ciertas operaciones por terminal.
    * **Objetivo:** Convertir el sistema en una solución robusta y completa, apta para ser liberada como producto interno general o incluso open source si se desease, que compita de tú a tú con Nexus/Artifactory en capacidades, a la vez que mantiene su ventaja de rendimiento y modernidad.

Cabe destacar que este roadmap es tentativo. Algunas funcionalidades podrían re-priorizarse según urgencia (por ejemplo, si desde el inicio se necesita Docker images, se adelantaría). Sin embargo, la planificación intenta primero cimentar bien la base (Maven/npm con RBAC y performance) antes de extenderse a otros dominios.

Cada hito irá acompañado de revisiones de este documento y de la documentación de usuario para reflejar exactamente el estado del sistema. De esta forma, el PRD evoluciona en paralelo al producto, manteniendo alineación entre lo planificado y lo implementado.

---

**Fuentes y referencias utilizadas:** Este documento se ha elaborado tomando en cuenta buenas prácticas y características conocidas de sistemas existentes similares. Por ejemplo, se consideraron comparativas entre Nexus, Artifactory y Archiva para identificar ventajas (como el uso de sistema de archivos para almacenamiento por rendimiento) y se incorporaron principios arquitectónicos reconocidos como la arquitectura hexagonal y los beneficios de la arquitectura dirigida por eventos. Asimismo, se fundamentó la elección de Kotlin/Native por sus beneficios de rendimiento y bajo consumo de recursos. Se han preservado las citas a dichas fuentes en el texto para referencia del equipo técnico. Esto asegura que las decisiones aquí plasmadas cuentan con respaldo teórico/práctico de la industria y literatura técnica reciente, fortaleciendo la justificación de este diseño.
