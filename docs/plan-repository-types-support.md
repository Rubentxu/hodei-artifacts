# Plan de Implementación - Repository Types Support (Story 5.2)

## 📋 **Resumen de la Historia de Usuario**

**Estado**: Draft → En Implementación  
**Objetivo**: Soporte completo para Maven, npm y Docker con compatibilidad 100% con clientes nativos.

## 🎯 **Criterios de Aceptación - Estado de Implementación**

| AC | Formato | Requisito | Estado | Implementación |
|----|---------|-----------|--------|----------------|
| 1 | Maven | Upload/download `.jar`, `.pom` | ✅ Planificado | Endpoints PUT/GET con paths Maven |
| 2 | Maven | Generar `maven-metadata.xml` | ✅ Planificado | Generador dinámico con versiones |
| 3 | npm | Publish/install `.tgz` | ✅ Planificado | Endpoints npm con tarballs |
| 4 | npm | Generar `package.json` repo | ✅ Planificado | Agregador de metadatos npm |
| 5 | Docker | Push/pull imágenes | ✅ Planificado | Registry V2 API completa |
| 6 | Todos | Clientes nativos | ✅ Planificado | APIs compatibles 100% |

## 🏗️ **Arquitectura de Implementación**

### **Estructura de Carpetas Propuesta**

```
crates/distribution/
├── src/
│   ├── domain/
│   │   ├── mod.rs
│   │   ├── format_handler.rs      # Trait base para manejadores
│   │   ├── maven/
│   │   │   ├── mod.rs
│   │   │   ├── maven_handler.rs   # Manejador Maven
│   │   │   ├── maven_metadata.rs  # Generador maven-metadata.xml
│   │   │   └── maven_paths.rs     # Parser de paths Maven
│   │   ├── npm/
│   │   │   ├── mod.rs
│   │   │   ├── npm_handler.rs     # Manejador npm
│   │   │   ├── npm_metadata.rs    # Generador package.json repo
│   │   │   └── npm_paths.rs       # Parser de paths npm
│   │   └── docker/
│   │       ├── mod.rs
│   │       ├── docker_handler.rs  # Manejador Docker
│   │       ├── docker_v2_api.rs   # Registry V2 API
│   │       └── docker_manifest.rs # Manejo de manifiestos OCI
│   ├── features/
│   │   ├── handle_maven_request/  # Feature VSA completa
│   │   ├── handle_npm_request/    # Feature VSA completa
│   │   └── handle_docker_request/ # Feature VSA completa
│   └── infrastructure/
│       ├── format_handlers/       # Implementaciones concretas
│       └── api/                   # Endpoints específicos
```

## 📊 **Especificaciones Técnicas por Formato**

### **1. Maven Repository Format**

**Estructura de Paths:**
```
/{groupId}/{artifactId}/{version}/{artifactId}-{version}.{extension}
/{groupId}/{artifactId}/maven-metadata.xml
```

**Endpoints Requeridos:**
```http
PUT /maven-repo/{groupId}/{artifactId}/{version}/{filename}
GET /maven-repo/{groupId}/{artifactId}/{version}/{filename}
GET /maven-repo/{groupId}/{artifactId}/maven-metadata.xml
```

**Formato maven-metadata.xml:**
```xml
<metadata>
  <groupId>com.example</groupId>
  <artifactId>my-app</artifactId>
  <versioning>
    <latest>1.2.3</latest>
    <release>1.2.3</release>
    <versions>
      <version>1.0.0</version>
      <version>1.1.0</version>
      <version>1.2.3</version>
    </versions>
    <lastUpdated>20240101120000</lastUpdated>
  </versioning>
</metadata>
```

### **2. npm Registry Format**

**Estructura de Paths:**
```
/{package}/-/{filename}
/{package}                    # Metadata del paquete
/{package}/{version}          # Metadata específica de versión
```

**Endpoints Requeridos:**
```http
PUT /npm-registry/{package}/-/{filename}
GET /npm-registry/{package}/-/{filename}
GET /npm-registry/{package}
GET /npm-registry/{package}/{version}
```

**Formato package.json del repositorio:**
```json
{
  "name": "my-package",
  "versions": {
    "1.0.0": { "dist": { "tarball": "..." } },
    "1.1.0": { "dist": { "tarball": "..." } }
  },
  "dist-tags": { "latest": "1.1.0" }
}
```

### **3. Docker Registry V2 API**

**Estructura de Paths (OCI Distribution Spec):**
```
/v2/{name}/manifests/{reference}
/v2/{name}/blobs/{digest}
/v2/{name}/blobs/uploads/
```

**Endpoints Requeridos:**
```http
PUT /v2/{name}/manifests/{reference}
GET /v2/{name}/manifests/{reference}
PUT /v2/{name}/blobs/{digest}
GET /v2/{name}/blobs/{digest}
POST /v2/{name}/blobs/uploads/
```

## 🔧 **Plan de Implementación por Fase**

### **Fase 1: Fundación (Semana 1)**

1. **Dominio Base**
   - ✅ Crear `domain/format_handler.rs` con trait base
   - ✅ Crear estructuras de dominio por formato
   - ✅ Definir errores específicos por formato

2. **Maven Handler Base**
   - ✅ Implementar `MavenFormatHandler`
   - ✅ Parser de paths Maven
   - ✅ Generador básico de maven-metadata.xml

3. **npm Handler Base**
   - ✅ Implementar `NpmFormatHandler`
   - ✅ Parser de paths npm
   - ✅ Generador básico de package.json repo

### **Fase 2: Maven Completo (Semana 2)**

1. **Feature Maven VSA**
   - ✅ `handle_maven_request` completa
   - ✅ Upload/download de artefactos
   - ✅ Generación dinámica de maven-metadata.xml
   - ✅ Tests unitarios extensivos

2. **Endpoints Maven**
   - ✅ Integración con API Gateway
   - ✅ Compatibilidad con Maven CLI
   - ✅ Tests de integración con Maven real

### **Fase 3: npm Completo (Semana 3)**

1. **Feature npm VSA**
   - ✅ `handle_npm_request` completa
   - ✅ Publish/install de paquetes
   - ✅ Generación dinámica de package.json repo
   - ✅ Tests unitarios extensivos

2. **Endpoints npm**
   - ✅ Integración con API Gateway
   - ✅ Compatibilidad con npm CLI
   - ✅ Tests de integración con npm real

### **Fase 4: Docker Completo (Semana 4)**

1. **Feature Docker VSA**
   - ✅ `handle_docker_request` completa
   - ✅ Registry V2 API implementation
   - ✅ Push/pull de imágenes
   - ✅ Tests unitarios extensivos

2. **Endpoints Docker**
   - ✅ Integración con API Gateway
   - ✅ Compatibilidad con Docker CLI
   - ✅ Tests de integración con Docker real

## 🧪 **Estrategia de Testing**

### **Tests Unitarios (Por Formato)**
- ✅ Parser de paths (cobertura 100%)
- ✅ Generadores de metadatos (todos los casos)
- ✅ Validaciones de formato (edge cases)
- ✅ Manejo de errores (todos los tipos)

### **Tests de Integración**
- ✅ Clientes nativos reales (Maven, npm, Docker)
- ✅ Flujos completos upload/download
- ✅ Generación de metadatos dinámicos
- ✅ Compatibilidad cross-formato

### **Tests E2E**
- ✅ Workflows completos con clientes CLI
- ✅ Performance y escalabilidad
- ✅ Casos de error y recuperación

## 📈 **Métricas de Éxito**

- ✅ **100% compatibilidad** con clientes nativos
- ✅ **< 100ms** latencia para metadatos
- ✅ **> 99.9%** uptime en tests de estrés
- ✅ **0 breaking changes** en APIs existentes
- ✅ **100% cobertura** de criterios de aceptación

## 🚀 **Próximos Pasos Inmediatos**

1. **Crear estructura base** del crate distribution
2. **Implementar trait FormatHandler** con métodos base
3. **Comenzar con Maven** (formato más establecido)
4. **Tests tempranos** con clientes reales
5. **Iterar y refinar** basado en feedback

## 📋 **Checklist de Validación**

- [ ] ¿Todas las APIs siguen las especificaciones oficiales?
- [ ] ¿Los clientes nativos funcionan sin configuración especial?
- [ ] ¿La generación de metadatos es atómica y consistente?
- [ ] ¿El manejo de errores proporciona información útil?
- [ ] ¿Los tests cubren todos los casos de uso reales?
- [ ] ¿La documentación es suficiente para desarrolladores?

---

**Nota**: Esta implementación seguirá estrictamente las especificaciones oficiales:
- **Maven**: [Maven Repository Layout](https://maven.apache.org/repository/layout.html)
- **npm**: [npm Registry API](https://github.com/npm/registry/blob/master/docs/REGISTRY-API.md)
- **Docker**: [OCI Distribution Spec](https://github.com/opencontainers/distribution-spec)

**¡Listo para comenzar la implementación!** 🚀