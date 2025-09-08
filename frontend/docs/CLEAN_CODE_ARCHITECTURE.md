# Arquitectura Clean Code para Frontend

## 📋 Resumen

Esta documentación describe la nueva arquitectura Clean Code implementada en el frontend de Hodei Artifacts, siguiendo principios SOLID y mejores prácticas de desarrollo de software.

## 🎯 Objetivos

1. **Separación de responsabilidades**: Cada componente tiene una única responsabilidad
2. **Principios SOLID**: Aplicar todos los principios de diseño orientado a objetos
3. **Testabilidad**: Facilitar el testing unitario y de integración
4. **Mantenibilidad**: Código más limpio y fácil de mantener
5. **Escalabilidad**: Arquitectura que crece con el proyecto

## 🏗️ Estructura de la Arquitectura

```
frontend/src/shared/
├── services/           # Servicios de dominio (lógica de negocio)
│   ├── repositories/   # Servicio de gestión de repositorios
│   │   ├── RepositoryService.ts      # Servicio principal
│   │   ├── ports/                    # Interfaces (contratos)
│   │   ├── adapters/                 # Implementaciones concretas
│   │   └── mappers/                  # Transformación de datos
│   └── search/         # Servicio de búsqueda
├── hooks/              # Hooks de React (capa de presentación)
│   ├── repositories/   # Hooks para repositorios
│   └── search/         # Hooks para búsqueda
└── types/              # Tipos TypeScript compartidos
```

## 🔧 Principios SOLID Aplicados

### 1. Single Responsibility Principle (SRP)
**Cada clase/componente tiene una única responsabilidad**

```typescript
// ❌ ANTES: Hook con múltiples responsabilidades
function useRepositories(filters) {
  // Maneja estado de filtros
  // Realiza peticiones HTTP
  // Transforma datos
  // Maneja errores
}

// ✅ DESPUÉS: Hooks separados por responsabilidad
function useRepositoryList(filters) { /* Solo consulta de datos */ }
function useRepositoryFilters(initialFilters) { /* Solo gestión de filtros */ }
function useCreateRepository() { /* Solo creación de repositorios */ }
```

### 2. Open/Closed Principle (OCP)
**Abierto para extensión, cerrado para modificación**

```typescript
// Puerto (interfaz) - No necesita cambios
interface RepositoryPort {
  obtenerRepositorios(filtros: RepositoryFilters): Promise<PaginatedResponse<Repository>>;
}

// Adaptadores - Se pueden agregar nuevos sin modificar el puerto
class OpenAPIRepositoryAdapter implements RepositoryPort { /* ... */ }
class MockRepositoryAdapter implements RepositoryPort { /* ... */ }
class LocalStorageRepositoryAdapter implements RepositoryPort { /* ... */ }
```

### 3. Liskov Substitution Principle (LSP)
**Los objetos de una clase derivada deben poder sustituir a objetos de la clase base**

```typescript
// Cualquier adaptador que implemente RepositoryPort puede ser usado
const repositoryService = new RepositoryService(
  new OpenAPIRepositoryAdapter() // o MockRepositoryAdapter, etc.
);
```

### 4. Interface Segregation Principle (ISP)
**Interfaces específicas y enfocadas**

```typescript
// ❌ ANTES: Interfaz genérica grande
interface APIClient {
  getRepositories(): Promise<any>;
  getUsers(): Promise<any>;
  searchPackages(): Promise<any>;
  // ... muchos métodos
}

// ✅ DESPUÉS: Interfaces específicas por dominio
interface RepositoryPort {
  obtenerRepositorios(filtros: RepositoryFilters): Promise<PaginatedResponse<Repository>>;
  obtenerRepositorio(id: string): Promise<Repository>;
  crearRepositorio(datos: CreateRepositoryRequest): Promise<Repository>;
}

interface SearchPort {
  buscarArtifatos(params: SearchArtifactsParams): Promise<SearchResults>;
}
```

### 5. Dependency Inversion Principle (DIP)
**Depender de abstracciones, no de implementaciones concretas**

```typescript
// ❌ ANTES: Dependencia directa de implementación
class RepositoryService {
  private api = new OpenAPIClient(); // Acoplado a implementación concreta
}

// ✅ DESPUÉS: Dependencia de abstracción
class RepositoryService {
  constructor(private readonly repositoryPort: RepositoryPort) {} // Inyección de dependencias
}
```

## 🔄 Flujo de Datos

```
Componente React → Hook → Servicio → Puerto → Adaptador → API/OpenAPI
```

### Ejemplo de uso:

```typescript
// 1. Componente usa el hook
function Dashboard() {
  const { data: repositories, isLoading } = useRepositoryList(filters);
  // ...
}

// 2. Hook usa el servicio
function useRepositoryList(filters: RepositoryFilters) {
  const repositoryService = useRepositoryService();
  return useQuery({
    queryKey: ['repositories'],
    queryFn: () => repositoryService.obtenerRepositoriosPaginados(filters)
  });
}

// 3. Servicio usa el puerto
class RepositoryService {
  async obtenerRepositoriosPaginados(filtros: RepositoryFilters) {
    return await this.repositoryPort.buscarRepositorios(filtros);
  }
}

// 4. Puerto es implementado por el adaptador
class OpenAPIRepositoryAdapter implements RepositoryPort {
  async buscarRepositorios(filtros: RepositoryFilters) {
    return await openAPIClient.listRepositories(/* ... */);
  }
}
```

## 🧪 Testing

### Ventajas para testing:

1. **Inyección de dependencias**: Facilita el mocking
2. **Interfaces claras**: Contratos bien definidos
3. **Separación de responsabilidades**: Testing unitario más simple
4. **Mappers aislados**: Fácil de testear transformaciones

### Ejemplo de testing:

```typescript
// Mock del adaptador
class MockRepositoryAdapter implements RepositoryPort {
  async buscarRepositorios() {
    return mockRepositories;
  }
}

// Test del servicio
describe('RepositoryService', () => {
  it('should return paginated repositories', async () => {
    const mockAdapter = new MockRepositoryAdapter();
    const service = new RepositoryService(mockAdapter);
    
    const result = await service.obtenerRepositoriosPaginados({});
    
    expect(result.data).toHaveLength(2);
    expect(result.total).toBe(2);
  });
});
```

## 🔄 Migración desde Código Legacy

### Fase 1: Implementar nueva arquitectura (✅ COMPLETADO)
- ✅ Crear servicios con puertos y adaptadores
- ✅ Crear hooks Clean Code
- ✅ Mantener hooks legacy para compatibilidad

### Fase 2: Actualizar componentes gradualmente (EN PROGRESO)
- 🔄 Crear versiones "Clean" de componentes principales
- 🔄 Usar hooks nuevos en nuevas funcionalidades
- 🔄 Mantener componentes legacy funcionando

### Fase 3: Deprecar código legacy (FUTURO)
- 📋 Marcar hooks legacy como obsoletos
- 📋 Actualizar documentación
- 📋 Migrar tests

## 📁 Archivos Clave Implementados

### Servicios
- `src/shared/services/repositories/RepositoryService.ts`
- `src/shared/services/repositories/ports/RepositoryPort.ts`
- `src/shared/services/repositories/adapters/OpenAPIRepositoryAdapter.ts`
- `src/shared/services/search/SearchService.ts`

### Hooks
- `src/shared/hooks/repositories/useRepositoryQueries.ts`
- `src/shared/hooks/repositories/useRepositoryMutations.ts`
- `src/shared/hooks/search/useSearchQueries.ts`

### Adaptadores Legacy
- `src/shared/hooks/repositories/legacyAdapter.ts`

## 🎯 Beneficios Obtenidos

1. **Código más limpio**: Nombres descriptivos en español siguiendo el dominio
2. **Mejor testabilidad**: Inyección de dependencias facilita el mocking
3. **Mayor flexibilidad**: Fácil cambiar implementaciones sin tocar lógica de negocio
4. **Mejor mantenibilidad**: Cada componente tiene responsabilidad única
5. **Compatibilidad**: Mantenemos funcionando el código existente durante la transición

## 🚀 Próximos Pasos

1. **Crear servicios para usuarios y autenticación**
2. **Implementar fábricas de servicios para testing**
3. **Actualizar componentes principales (Dashboard, Repositories, etc.)**
4. **Crear tests unitarios para los nuevos servicios**
5. **Documentar casos de uso específicos**

## 📚 Referencias

- [Clean Architecture by Robert C. Martin](https://blog.cleancoder.com/uncle-bob/2012/08/13/the-clean-architecture.html)
- [SOLID Principles](https://en.wikipedia.org/wiki/SOLID)
- [Ports and Adapters Pattern](https://alistair.cockburn.us/hexagonal-architecture/)
- [React Query Best Practices](https://tanstack.com/query/latest/docs/react/guides/best-practices)

---

**Nota**: Esta arquitectura está en proceso de implementación. Algunos componentes aún usan el código legacy mientras se completa la migración.