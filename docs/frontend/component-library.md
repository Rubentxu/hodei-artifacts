# Component Library

## Biblioteca de Componentes - Hodei Artifacts

Esta documentación describe la biblioteca de componentes reutilizables siguiendo los principios de Atomic Design, organizados en atoms, molecules, organisms y templates.

## Filosofía de Diseño

### Principios Fundamentales
1. **Consistency**: Componentes consistentes en comportamiento y apariencia
2. **Accessibility**: Cumplimiento WCAG 2.1 AA en todos los componentes
3. **Reusability**: Componentes altamente reutilizables y configurables
4. **Type Safety**: Tipado estricto con TypeScript para prevenir errores
5. **Performance**: Optimizados para rendering eficiente

### Convenciones de Naming
- **Props**: Descriptivos y consistentes (`isLoading` vs `loading`)
- **Variants**: Usando enums o tipos unión (`'primary' | 'secondary'`)
- **Event Handlers**: Prefijo `on` (`onClick`, `onSubmit`, `onChange`)
- **Boolean Props**: Prefijo `is`, `has`, `can` (`isDisabled`, `hasError`)

## Atoms (Componentes Básicos)

### Button

**Propósito**: Componente básico de botón con múltiples variantes y estados.

```typescript
interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: 'primary' | 'secondary' | 'ghost' | 'danger'
  size?: 'sm' | 'md' | 'lg'
  isLoading?: boolean
  leftIcon?: React.ReactNode
  rightIcon?: React.ReactNode
}

const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ variant = 'primary', size = 'md', isLoading = false, leftIcon, rightIcon, children, className, ...props }, ref) => {
    return (
      <button
        ref={ref}
        className={cn(
          buttonVariants({ variant, size }),
          isLoading && 'cursor-not-allowed opacity-60',
          className
        )}
        disabled={isLoading || props.disabled}
        {...props}
      >
        {leftIcon && <span className="mr-2">{leftIcon}</span>}
        {isLoading && <Spinner size="sm" className="mr-2" />}
        {children}
        {rightIcon && <span className="ml-2">{rightIcon}</span>}
      </button>
    )
  }
)

Button.displayName = 'Button'
```

**Variantes**:
- `primary`: Acción principal (azul)
- `secondary`: Acción secundaria (outline)
- `ghost`: Acción terciaria (transparente)
- `danger`: Acciones destructivas (rojo)

**Estados**:
- Normal, hover, focus, active, disabled, loading

**Ejemplo de uso**:
```tsx
<Button variant="primary" size="md" onClick={handleSubmit} isLoading={isSubmitting}>
  Save Repository
</Button>
```

---

### Input

**Propósito**: Campo de entrada básico con soporte para diferentes tipos y estados.

```typescript
interface InputProps extends InputHTMLAttributes<HTMLInputElement> {
  variant?: 'default' | 'error' | 'success'
  size?: 'sm' | 'md' | 'lg'
  leftIcon?: React.ReactNode
  rightIcon?: React.ReactNode
  isLoading?: boolean
}

const Input = forwardRef<HTMLInputElement, InputProps>(
  ({ variant = 'default', size = 'md', leftIcon, rightIcon, isLoading = false, className, ...props }, ref) => {
    return (
      <div className="relative">
        {leftIcon && (
          <div className="absolute left-3 top-1/2 transform -translate-y-1/2 text-gray-400">
            {leftIcon}
          </div>
        )}
        <input
          ref={ref}
          className={cn(
            inputVariants({ variant, size }),
            leftIcon && 'pl-10',
            rightIcon && 'pr-10',
            className
          )}
          disabled={isLoading}
          {...props}
        />
        {(rightIcon || isLoading) && (
          <div className="absolute right-3 top-1/2 transform -translate-y-1/2 text-gray-400">
            {isLoading ? <Spinner size="xs" /> : rightIcon}
          </div>
        )}
      </div>
    )
  }
)

Input.displayName = 'Input'
```

**Características**:
- Soporte para todos los tipos HTML5
- Estados visuales para error/success
- Iconos izquierda/derecha
- Loading state integrado
- Placeholder animado

**Ejemplo de uso**:
```tsx
<Input
  type="email"
  placeholder="user@company.com"
  variant={errors.email ? 'error' : 'default'}
  leftIcon={<MailIcon />}
/>
```

---

### Badge

**Propósito**: Indicador visual para estados, categorías o metadatos.

```typescript
interface BadgeProps {
  variant?: 'default' | 'primary' | 'success' | 'warning' | 'error'
  size?: 'sm' | 'md' | 'lg'
  children: React.ReactNode
}
```

**Variantes por contexto**:
- Tipos de repositorio: `Maven`, `npm`, `PyPI`
- Estados: `Active`, `Inactive`, `Pending`
- Tamaños: `12 KB`, `2.3 MB`, `156 GB`

**Ejemplo de uso**:
```tsx
<Badge variant="success">Active</Badge>
<Badge variant="primary">Maven</Badge>
```

---

### Icon

**Propósito**: Wrapper para iconos con tamaños y colores consistentes.

```typescript
interface IconProps {
  name: string
  size?: 'xs' | 'sm' | 'md' | 'lg' | 'xl'
  color?: string
  className?: string
}
```

**Iconos disponibles**:
- Navegación: `home`, `search`, `settings`, `user`
- Acciones: `upload`, `download`, `edit`, `delete`, `copy`
- Estados: `loading`, `success`, `error`, `warning`
- Tipos: `maven`, `npm`, `pypi`, `docker`, `folder`, `file`

---

### Spinner

**Propósito**: Indicador de carga con diferentes tamaños.

```typescript
interface SpinnerProps {
  size?: 'xs' | 'sm' | 'md' | 'lg'
  color?: string
  className?: string
}
```

## Molecules (Combinaciones Simples)

### FormField

**Propósito**: Wrapper que combina Label + Input + ErrorMessage.

```typescript
interface FormFieldProps {
  label: string
  error?: string
  hint?: string
  required?: boolean
  children: React.ReactElement
}
```

**Características**:
- Asociación automática label-input
- Manejo de estados de error
- Texto de ayuda opcional
- Indicador de campo requerido

**Ejemplo de uso**:
```tsx
<FormField
  label="Repository Name"
  error={errors.name}
  hint="Use lowercase letters and hyphens"
  required
>
  <Input {...register('name')} />
</FormField>
```

---

### SearchBox

**Propósito**: Campo de búsqueda con debounce y sugerencias.

```typescript
interface SearchBoxProps {
  value: string
  onChange: (value: string) => void
  onSearch: (query: string) => void
  placeholder?: string
  suggestions?: string[]
  isLoading?: boolean
  debounceMs?: number
}
```

**Características**:
- Debounce automático
- Dropdown de sugerencias
- Loading state
- Clear button
- Keyboard navigation

**Ejemplo de uso**:
```tsx
<SearchBox
  value={query}
  onChange={setQuery}
  onSearch={handleSearch}
  placeholder="Search repositories..."
  suggestions={recentSearches}
  isLoading={isSearching}
/>
```

---

### Card

**Propósito**: Contenedor flexible para mostrar información agrupada.

```typescript
interface CardProps {
  variant?: 'default' | 'elevated' | 'outlined'
  padding?: 'sm' | 'md' | 'lg'
  hover?: boolean
  children: React.ReactNode
}
```

**Subcomponentes**:
- `Card.Header`: Encabezado con título y acciones
- `Card.Body`: Contenido principal
- `Card.Footer`: Pie con botones o metadatos

**Ejemplo de uso**:
```tsx
<Card variant="elevated" hover>
  <Card.Header>
    <h3>maven-central</h3>
    <Button variant="ghost" size="sm">⋯</Button>
  </Card.Header>
  <Card.Body>
    <p>Maven repository for OSS packages</p>
    <Badge variant="success">1,234 packages</Badge>
  </Card.Body>
</Card>
```

---

### Pagination

**Propósito**: Navegación entre páginas con información de estado.

```typescript
interface PaginationProps {
  currentPage: number
  totalPages: number
  totalItems: number
  itemsPerPage: number
  onPageChange: (page: number) => void
  showInfo?: boolean
}
```

**Características**:
- Botones anterior/siguiente
- Números de página con elipsis
- Información de items mostrados
- Navegación por teclado

---

## Organisms (Componentes Complejos)

### DataTable

**Propósito**: Tabla avanzada con sorting, filtering y paginación.

```typescript
interface Column<T> {
  key: keyof T
  title: string
  sortable?: boolean
  render?: (value: any, row: T) => React.ReactNode
  width?: string
}

interface DataTableProps<T> {
  data: T[]
  columns: Column<T>[]
  loading?: boolean
  pagination?: PaginationProps
  onSort?: (column: keyof T, direction: 'asc' | 'desc') => void
  onRowClick?: (row: T) => void
  selection?: {
    selectedRows: T[]
    onSelectionChange: (rows: T[]) => void
  }
}
```

**Características**:
- Ordenamiento por columnas
- Selección múltiple con checkboxes
- Loading states con skeleton
- Acciones por fila
- Responsive en móviles

**Ejemplo de uso**:
```tsx
<DataTable
  data={repositories}
  columns={repositoryColumns}
  loading={isLoading}
  pagination={paginationConfig}
  onSort={handleSort}
  onRowClick={navigateToRepository}
  selection={{
    selectedRows: selectedRepos,
    onSelectionChange: setSelectedRepos
  }}
/>
```

---

### Modal

**Propósito**: Overlay modal con backdrop y gestión de foco.

```typescript
interface ModalProps {
  isOpen: boolean
  onClose: () => void
  title?: string
  size?: 'sm' | 'md' | 'lg' | 'xl'
  children: React.ReactNode
  footer?: React.ReactNode
}
```

**Características**:
- Gestión automática de foco
- Cierre con ESC o clic fuera
- Tamaños predefinidos
- Portal para rendering fuera del DOM
- Animaciones de entrada/salida

**Ejemplo de uso**:
```tsx
<Modal
  isOpen={isCreateModalOpen}
  onClose={closeModal}
  title="Create Repository"
  size="md"
  footer={
    <>
      <Button variant="secondary" onClick={closeModal}>Cancel</Button>
      <Button variant="primary" onClick={handleSubmit}>Create</Button>
    </>
  }
>
  <RepositoryForm />
</Modal>
```

---

### Header

**Propósito**: Encabezado global con navegación y usuario.

```typescript
interface HeaderProps {
  user: User | null
  onSearch: (query: string) => void
  onLogout: () => void
}
```

**Características**:
- Logo y navegación principal
- SearchBox global
- Menú de usuario con dropdown
- Breadcrumbs contextuales
- Responsive con menu hamburguesa

---

### Sidebar

**Propósito**: Navegación lateral con menú jerárquico.

```typescript
interface NavigationItem {
  id: string
  label: string
  icon: string
  path: string
  children?: NavigationItem[]
  badge?: string | number
}

interface SidebarProps {
  items: NavigationItem[]
  collapsed?: boolean
  onToggle?: () => void
}
```

**Características**:
- Navegación jerárquica expandible
- Estado activo visual
- Badges para notificaciones
- Colapsar/expandir
- Navegación por teclado

---

## Templates (Layouts de Página)

### MainLayout

**Propósito**: Layout principal con header, sidebar y área de contenido.

```typescript
interface MainLayoutProps {
  children: React.ReactNode
  sidebar?: React.ReactNode
  header?: React.ReactNode
}
```

**Características**:
- Responsive design con breakpoints
- Sidebar colapsable en móviles
- Área de contenido con scroll independiente
- Footer opcional

---

### AuthLayout

**Propósito**: Layout para páginas de autenticación.

```typescript
interface AuthLayoutProps {
  children: React.ReactNode
  title?: string
  subtitle?: string
}
```

**Características**:
- Centrado vertical y horizontal
- Fondo con branding
- Responsive para todos los dispositivos

---

## Specialized Components

### FileUpload

**Propósito**: Componente de upload con drag & drop.

```typescript
interface FileUploadProps {
  accept?: string
  multiple?: boolean
  maxSize?: number
  onFilesSelected: (files: File[]) => void
  onUploadProgress?: (progress: number) => void
  children?: React.ReactNode
}
```

**Características**:
- Drag & drop zone
- Validación de tipos y tamaños
- Progress bar por archivo
- Preview de archivos seleccionados
- Cancelación de uploads

---

### CodeEditor

**Propósito**: Editor de código para políticas ABAC.

```typescript
interface CodeEditorProps {
  value: string
  onChange: (value: string) => void
  language?: 'cedar' | 'json' | 'yaml'
  readOnly?: boolean
  height?: string
}
```

**Características**:
- Syntax highlighting
- Autocompletado básico
- Líneas de error
- Tema consistente con la aplicación

---

### BreadcrumbNavigation

**Propósito**: Navegación de contexto jerárquico.

```typescript
interface BreadcrumbItem {
  label: string
  path?: string
}

interface BreadcrumbNavigationProps {
  items: BreadcrumbItem[]
  separator?: React.ReactNode
}
```

---

## Patterns de Composición

### Compound Components

Para componentes complejos, utilizamos el patrón de compound components:

```tsx
// Uso con compound components
<DataTable data={data}>
  <DataTable.Header>
    <DataTable.Column key="name" sortable>Name</DataTable.Column>
    <DataTable.Column key="type">Type</DataTable.Column>
    <DataTable.Column key="size">Size</DataTable.Column>
  </DataTable.Header>
  <DataTable.Body>
    {/* Renderizado automático */}
  </DataTable.Body>
</DataTable>
```

### Render Props

Para máxima flexibilidad en componentes de datos:

```tsx
<ArtifactList repositoryId={id}>
  {({ artifacts, loading, error }) => (
    <div>
      {loading && <Spinner />}
      {error && <ErrorMessage error={error} />}
      {artifacts.map(artifact => (
        <ArtifactCard key={artifact.id} artifact={artifact} />
      ))}
    </div>
  )}
</ArtifactList>
```

### Custom Hooks Integration

Cada componente complejo tiene su hook correspondiente, siguiendo el patrón de custom hooks de React:

```tsx
// Hook para gestión de tabla con estado local
const useDataTable = <T>(data: T[], options: DataTableOptions) => {
  const [sortConfig, setSortConfig] = useState<SortConfig | null>(null)
  const [selectedRows, setSelectedRows] = useState<T[]>([])
  const [filters, setFilters] = useState<Record<string, any>>({})
  
  const sortedData = useMemo(() => {
    if (!sortConfig) return data
    
    return [...data].sort((a, b) => {
      const aValue = a[sortConfig.key]
      const bValue = b[sortConfig.key]
      
      if (aValue < bValue) return sortConfig.direction === 'asc' ? -1 : 1
      if (aValue > bValue) return sortConfig.direction === 'asc' ? 1 : -1
      return 0
    })
  }, [data, sortConfig])
  
  const handleSort = useCallback((column: keyof T) => {
    setSortConfig(current => ({
      key: column,
      direction: current?.key === column && current.direction === 'asc' ? 'desc' : 'asc'
    }))
  }, [])
  
  const handleSelectRow = useCallback((row: T) => {
    setSelectedRows(current => {
      const isSelected = current.includes(row)
      return isSelected 
        ? current.filter(r => r !== row)
        : [...current, row]
    })
  }, [])
  
  const handleSelectAll = useCallback(() => {
    setSelectedRows(current => 
      current.length === sortedData.length ? [] : sortedData
    )
  }, [sortedData])
  
  return {
    sortedData,
    sortConfig,
    selectedRows,
    filters,
    handleSort,
    handleSelectRow,
    handleSelectAll,
    setFilters
  }
}

// Hook para gestión de modal con estado
const useModal = () => {
  const [isOpen, setIsOpen] = useState(false)
  
  const openModal = useCallback(() => setIsOpen(true), [])
  const closeModal = useCallback(() => setIsOpen(false), [])
  const toggleModal = useCallback(() => setIsOpen(prev => !prev), [])
  
  return { isOpen, openModal, closeModal, toggleModal }
}

// Hook para gestión de formularios con validación
const useForm = <T extends Record<string, any>>(
  initialValues: T,
  validationSchema?: (values: T) => Record<string, string>
) => {
  const [values, setValues] = useState<T>(initialValues)
  const [errors, setErrors] = useState<Record<string, string>>({})
  const [isSubmitting, setIsSubmitting] = useState(false)
  
  const handleChange = useCallback((name: keyof T, value: any) => {
    setValues(prev => ({ ...prev, [name]: value }))
    
    // Clear error when user starts typing
    if (errors[name as string]) {
      setErrors(prev => ({ ...prev, [name]: undefined }))
    }
  }, [errors])
  
  const validate = useCallback(() => {
    if (!validationSchema) return true
    
    const newErrors = validationSchema(values)
    setErrors(newErrors)
    return Object.keys(newErrors).length === 0
  }, [values, validationSchema])
  
  const handleSubmit = useCallback(async (
    onSubmit: (values: T) => Promise<void> | void
  ) => {
    if (!validate()) return
    
    try {
      setIsSubmitting(true)
      await onSubmit(values)
    } finally {
      setIsSubmitting(false)
    }
  }, [values, validate])
  
  const reset = useCallback(() => {
    setValues(initialValues)
    setErrors({})
    setIsSubmitting(false)
  }, [initialValues])
  
  return {
    values,
    errors,
    isSubmitting,
    handleChange,
    handleSubmit,
    validate,
    reset
  }
}

// Uso en componente
const RepositoryTable = () => {
  const { data: repositories, isLoading } = useRepositories()
  const { 
    sortedData, 
    selectedRows, 
    handleSort, 
    handleSelectRow, 
    handleSelectAll 
  } = useDataTable(repositories || [], {})
  
  return (
    <DataTable
      data={sortedData}
      onSort={handleSort}
      selection={{
        selectedRows,
        onSelectionChange: handleSelectRow,
        onSelectAll: handleSelectAll
      }}
      loading={isLoading}
    />
  )
}
```

## Testing Strategy

### Component Testing

Cada componente incluye tests unitarios completos siguiendo las mejores prácticas de React Testing Library:

```typescript
// Button.test.tsx
import { render, screen, fireEvent } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { Button } from './Button'

describe('Button', () => {
  it('renders with correct variant class', () => {
    render(<Button variant="primary">Click me</Button>)
    const button = screen.getByRole('button', { name: /click me/i })
    expect(button).toHaveClass('bg-primary-600')
  })

  it('shows loading state correctly', () => {
    render(<Button isLoading>Loading</Button>)
    const button = screen.getByRole('button')
    expect(button).toBeDisabled()
    expect(button).toHaveClass('cursor-not-allowed', 'opacity-60')
    expect(screen.getByTestId('spinner')).toBeInTheDocument()
  })

  it('handles click events', async () => {
    const user = userEvent.setup()
    const handleClick = vi.fn()
    render(<Button onClick={handleClick}>Click me</Button>)
    
    await user.click(screen.getByRole('button'))
    expect(handleClick).toHaveBeenCalledTimes(1)
  })

  it('renders with left and right icons', () => {
    const LeftIcon = () => <span data-testid="left-icon">←</span>
    const RightIcon = () => <span data-testid="right-icon">→</span>
    
    render(
      <Button leftIcon={<LeftIcon />} rightIcon={<RightIcon />}>
        With Icons
      </Button>
    )
    
    expect(screen.getByTestId('left-icon')).toBeInTheDocument()
    expect(screen.getByTestId('right-icon')).toBeInTheDocument()
  })

  it('forwards ref correctly', () => {
    const ref = createRef<HTMLButtonElement>()
    render(<Button ref={ref}>Button</Button>)
    
    expect(ref.current).toBeInstanceOf(HTMLButtonElement)
    expect(ref.current?.tagName).toBe('BUTTON')
  })
})

// DataTable.test.tsx
describe('DataTable', () => {
  const mockData = [
    { id: '1', name: 'Repo 1', type: 'maven' },
    { id: '2', name: 'Repo 2', type: 'npm' }
  ]
  
  const mockColumns = [
    { key: 'name' as const, title: 'Name', sortable: true },
    { key: 'type' as const, title: 'Type' }
  ]

  it('renders table with data', () => {
    render(<DataTable data={mockData} columns={mockColumns} />)
    
    expect(screen.getByRole('table')).toBeInTheDocument()
    expect(screen.getByText('Repo 1')).toBeInTheDocument()
    expect(screen.getByText('maven')).toBeInTheDocument()
  })

  it('handles sorting when column is clicked', async () => {
    const user = userEvent.setup()
    const handleSort = vi.fn()
    
    render(
      <DataTable 
        data={mockData} 
        columns={mockColumns} 
        onSort={handleSort}
      />
    )
    
    await user.click(screen.getByText('Name'))
    expect(handleSort).toHaveBeenCalledWith('name', 'asc')
  })

  it('shows loading skeleton when loading', () => {
    render(<DataTable data={[]} columns={mockColumns} loading />)
    
    expect(screen.getByTestId('table-skeleton')).toBeInTheDocument()
  })
})
```

### Accessibility Testing

Tests de accesibilidad exhaustivos para cumplir WCAG 2.1 AA:

```typescript
// Modal.accessibility.test.tsx
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { axe, toHaveNoViolations } from 'jest-axe'
import { Modal } from './Modal'

expect.extend(toHaveNoViolations)

describe('Modal accessibility', () => {
  it('should not have accessibility violations', async () => {
    const { container } = render(
      <Modal isOpen title="Test Modal">
        <input aria-label="Test input" />
        <button>Test button</button>
      </Modal>
    )
    
    const results = await axe(container)
    expect(results).toHaveNoViolations()
  })

  it('traps focus within modal', async () => {
    const user = userEvent.setup()
    
    render(
      <>
        <button>Outside button</button>
        <Modal isOpen title="Test Modal">
          <input data-testid="first-input" />
          <button data-testid="modal-button">Modal button</button>
          <input data-testid="last-input" />
        </Modal>
      </>
    )
    
    // Focus should be trapped in modal
    const firstInput = screen.getByTestId('first-input')
    const lastInput = screen.getByTestId('last-input')
    
    // Tab should cycle within modal
    await user.tab()
    expect(screen.getByTestId('modal-button')).toHaveFocus()
    
    await user.tab()
    expect(lastInput).toHaveFocus()
    
    await user.tab()
    expect(firstInput).toHaveFocus() // Should cycle back
  })

  it('has correct ARIA attributes', () => {
    render(
      <Modal 
        isOpen 
        title="Test Modal" 
        onClose={() => {}}
      >
        Content
      </Modal>
    )
    
    const dialog = screen.getByRole('dialog')
    expect(dialog).toHaveAttribute('aria-labelledby')
    expect(dialog).toHaveAttribute('aria-modal', 'true')
    expect(dialog).toHaveAttribute('tabindex', '-1')
  })

  it('closes on escape key', async () => {
    const user = userEvent.setup()
    const onClose = vi.fn()
    
    render(
      <Modal isOpen onClose={onClose}>
        Content
      </Modal>
    )
    
    await user.keyboard('{Escape}')
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('announces content to screen readers', () => {
    render(
      <Modal isOpen title="Important Update">
        <p>Your changes have been saved.</p>
      </Modal>
    )
    
    expect(screen.getByText('Important Update')).toBeInTheDocument()
    expect(screen.getByRole('dialog')).toHaveTextContent('Your changes have been saved.')
  })
})

// FormField.accessibility.test.tsx
describe('FormField accessibility', () => {
  it('associates label with input correctly', () => {
    render(
      <FormField label="Email address" required>
        <Input type="email" />
      </FormField>
    )
    
    const input = screen.getByLabelText('Email address *')
    expect(input).toBeInTheDocument()
    expect(input).toHaveAttribute('required')
  })

  it('provides error announcement for screen readers', () => {
    render(
      <FormField label="Password" error="Password is too short">
        <Input type="password" />
      </FormField>
    )
    
    const input = screen.getByLabelText('Password')
    expect(input).toHaveAttribute('aria-invalid', 'true')
    expect(input).toHaveAttribute('aria-describedby')
    
    const errorMessage = screen.getByText('Password is too short')
    expect(errorMessage).toHaveAttribute('role', 'alert')
  })
})
```

### Hook Testing

Testing de custom hooks con React Testing Library:

```typescript
// useDataTable.test.ts
import { renderHook, act } from '@testing-library/react'
import { useDataTable } from './useDataTable'

const mockData = [
  { id: '1', name: 'Item A', value: 100 },
  { id: '2', name: 'Item B', value: 50 }
]

describe('useDataTable', () => {
  it('sorts data correctly', () => {
    const { result } = renderHook(() => useDataTable(mockData, {}))
    
    act(() => {
      result.current.handleSort('name')
    })
    
    expect(result.current.sortConfig).toEqual({ 
      key: 'name', 
      direction: 'asc' 
    })
    expect(result.current.sortedData[0].name).toBe('Item A')
  })

  it('handles row selection', () => {
    const { result } = renderHook(() => useDataTable(mockData, {}))
    
    act(() => {
      result.current.handleSelectRow(mockData[0])
    })
    
    expect(result.current.selectedRows).toContain(mockData[0])
  })
})
```

### Visual Regression Testing

Usando Storybook y Chromatic para detectar cambios visuales no intencionados:

```typescript
// visual-regression.test.ts
import { test, expect } from '@playwright/test'

test.describe('Visual Regression Tests', () => {
  test('Button component matches screenshot', async ({ page }) => {
    await page.goto('/storybook/?path=/story/atoms-button--primary')
    await expect(page.locator('[data-testid="button-primary"]')).toHaveScreenshot('button-primary.png')
  })

  test('DataTable component matches screenshot', async ({ page }) => {
    await page.goto('/storybook/?path=/story/organisms-datatable--default')
    await expect(page.locator('[data-testid="data-table"]')).toHaveScreenshot('data-table-default.png')
  })
})
```

## Documentation & Storybook

Cada componente tiene documentación completa en Storybook con ejemplos interactivos y controles:

### Configuración de Storybook

```typescript
// .storybook/main.ts
import type { StorybookConfig } from '@storybook/react-vite'

const config: StorybookConfig = {
  stories: ['../src/**/*.stories.@(js|jsx|ts|tsx|mdx)'],
  addons: [
    '@storybook/addon-essentials',
    '@storybook/addon-a11y',
    '@storybook/addon-design-tokens',
    '@storybook/addon-docs'
  ],
  framework: {
    name: '@storybook/react-vite',
    options: {}
  },
  typescript: {
    check: false,
    reactDocgen: 'react-docgen-typescript'
  }
}

export default config
```

### Documentación de Componentes

```typescript
// Button.stories.tsx
import type { Meta, StoryObj } from '@storybook/react'
import { Button } from './Button'
import { PlusIcon, LoaderIcon } from '@/shared/icons'

const meta: Meta<typeof Button> = {
  title: 'Design System/Atoms/Button',
  component: Button,
  parameters: {
    layout: 'centered',
    docs: {
      description: {
        component: 'Botón reutilizable con múltiples variantes y estados.'
      }
    }
  },
  argTypes: {
    variant: {
      control: { type: 'select' },
      options: ['primary', 'secondary', 'ghost', 'danger'],
      description: 'Variante visual del botón'
    },
    size: {
      control: { type: 'select' },
      options: ['sm', 'md', 'lg'],
      description: 'Tamaño del botón'
    },
    isLoading: {
      control: 'boolean',
      description: 'Estado de carga que deshabilita el botón'
    }
  },
  tags: ['autodocs']
}

export default meta
type Story = StoryObj<typeof Button>

export const Primary: Story = {
  args: {
    variant: 'primary',
    children: 'Primary Button'
  }
}

export const Secondary: Story = {
  args: {
    variant: 'secondary',
    children: 'Secondary Button'
  }
}

export const WithLeftIcon: Story = {
  args: {
    variant: 'primary',
    leftIcon: <PlusIcon />,
    children: 'Add Repository'
  }
}

export const Loading: Story = {
  args: {
    variant: 'primary',
    isLoading: true,
    children: 'Saving...'
  }
}

export const AllSizes: Story = {
  render: () => (
    <div className="flex items-center gap-4">
      <Button size="sm">Small</Button>
      <Button size="md">Medium</Button>
      <Button size="lg">Large</Button>
    </div>
  )
}

export const AllVariants: Story = {
  render: () => (
    <div className="flex flex-col gap-4">
      <div className="flex gap-4">
        <Button variant="primary">Primary</Button>
        <Button variant="secondary">Secondary</Button>
        <Button variant="ghost">Ghost</Button>
        <Button variant="danger">Danger</Button>
      </div>
      <div className="flex gap-4">
        <Button variant="primary" disabled>Disabled Primary</Button>
        <Button variant="secondary" disabled>Disabled Secondary</Button>
        <Button variant="ghost" disabled>Disabled Ghost</Button>
        <Button variant="danger" disabled>Disabled Danger</Button>
      </div>
    </div>
  )
}
```

## Modern React Patterns

### Error Boundaries para Components

```typescript
// ComponentErrorBoundary.tsx
interface ComponentErrorBoundaryProps {
  children: React.ReactNode
  fallback?: React.ComponentType<{ error: Error; retry: () => void }>
  onError?: (error: Error, errorInfo: ErrorInfo) => void
}

const ComponentErrorBoundary: React.FC<ComponentErrorBoundaryProps> = ({
  children,
  fallback: Fallback,
  onError
}) => {
  return (
    <ErrorBoundary
      FallbackComponent={Fallback || DefaultErrorFallback}
      onError={onError}
      onReset={() => window.location.reload()}
    >
      {children}
    </ErrorBoundary>
  )
}

// Uso en componentes complejos
<ComponentErrorBoundary>
  <DataTable data={repositories} columns={columns} />
</ComponentErrorBoundary>
```

### Suspense para Lazy Loading

```typescript
// Lazy loading de componentes pesados
const DataVisualization = lazy(() => import('./DataVisualization'))
const CodeEditor = lazy(() => import('./CodeEditor'))

// Componente con Suspense
const Dashboard = () => {
  const [showChart, setShowChart] = useState(false)
  
  return (
    <div>
      <h1>Dashboard</h1>
      {showChart && (
        <Suspense fallback={<ChartSkeleton />}>
          <DataVisualization />
        </Suspense>
      )}
    </div>
  )
}
```

### Context API para Component Communication

```typescript
// DataTableContext para compound components
interface DataTableContextValue {
  data: any[]
  selectedRows: any[]
  onSort: (column: string) => void
  onSelect: (row: any) => void
}

const DataTableContext = createContext<DataTableContextValue | undefined>(undefined)

const useDataTableContext = () => {
  const context = useContext(DataTableContext)
  if (!context) {
    throw new Error('useDataTableContext must be used within DataTable')
  }
  return context
}

// Implementación con Context
const DataTable = ({ children, data, ...props }) => {
  const tableState = useDataTable(data, props)
  
  return (
    <DataTableContext.Provider value={tableState}>
      <table className="data-table">
        {children}
      </table>
    </DataTableContext.Provider>
  )
}

// Subcomponentes que usan el context
const DataTableHeader = ({ children }) => {
  const { onSort } = useDataTableContext()
  return <thead>{children}</thead>
}

const DataTableColumn = ({ sortable, children, column }) => {
  const { onSort } = useDataTableContext()
  
  return (
    <th 
      onClick={sortable ? () => onSort(column) : undefined}
      className={sortable ? 'cursor-pointer' : ''}
    >
      {children}
    </th>
  )
}
```

### Performance Optimization Patterns

```typescript
// Memoización inteligente
const RepositoryCard = memo(({ repository, onEdit, onDelete }) => {
  return (
    <Card>
      <Card.Header>{repository.name}</Card.Header>
      <Card.Body>{repository.description}</Card.Body>
      <Card.Footer>
        <Button onClick={() => onEdit(repository.id)}>Edit</Button>
        <Button onClick={() => onDelete(repository.id)}>Delete</Button>
      </Card.Footer>
    </Card>
  )
}, (prevProps, nextProps) => {
  // Custom comparison
  return prevProps.repository.id === nextProps.repository.id &&
         prevProps.repository.updatedAt === nextProps.repository.updatedAt
})

// Callbacks estables
const RepositoryList = ({ repositories }) => {
  const handleEdit = useCallback((id: string) => {
    // Edit logic
  }, [])
  
  const handleDelete = useCallback((id: string) => {
    // Delete logic
  }, [])
  
  return (
    <div>
      {repositories.map(repo => (
        <RepositoryCard
          key={repo.id}
          repository={repo}
          onEdit={handleEdit}
          onDelete={handleDelete}
        />
      ))}
    </div>
  )
}

// Virtual scrolling para listas grandes
const VirtualizedList = ({ items, itemHeight = 50 }) => {
  const [scrollTop, setScrollTop] = useState(0)
  const containerHeight = 400
  
  const visibleItems = useMemo(() => {
    const startIndex = Math.floor(scrollTop / itemHeight)
    const endIndex = Math.min(
      startIndex + Math.ceil(containerHeight / itemHeight) + 5,
      items.length
    )
    
    return items.slice(startIndex, endIndex).map((item, index) => ({
      item,
      index: startIndex + index,
      top: (startIndex + index) * itemHeight
    }))
  }, [items, scrollTop, itemHeight])
  
  return (
    <div 
      style={{ height: containerHeight, overflow: 'auto' }}
      onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)}
    >
      <div style={{ height: items.length * itemHeight, position: 'relative' }}>
        {visibleItems.map(({ item, index, top }) => (
          <div
            key={item.id}
            style={{
              position: 'absolute',
              top,
              height: itemHeight,
              width: '100%'
            }}
          >
            <RepositoryCard repository={item} />
          </div>
        ))}
      </div>
    </div>
  )
}
```

### Type-Safe Component Props

```typescript
// Discriminated unions para props
type ButtonProps = 
  | {
      variant: 'primary' | 'secondary'
      isLoading?: boolean
      onClick: () => void
    }
  | {
      variant: 'link'
      href: string
      external?: boolean
    }

// Generic components con constraints
interface DataDisplayProps<T> {
  data: T[]
  keyExtractor: (item: T) => string
  renderItem: (item: T) => React.ReactNode
  emptyState?: React.ReactNode
}

const DataDisplay = <T,>({
  data,
  keyExtractor,
  renderItem,
  emptyState
}: DataDisplayProps<T>) => {
  if (data.length === 0) {
    return <>{emptyState || <EmptyState />}</>
  }
  
  return (
    <div>
      {data.map(item => (
        <div key={keyExtractor(item)}>
          {renderItem(item)}
        </div>
      ))}
    </div>
  )
}

// Uso type-safe
<DataDisplay
  data={repositories}
  keyExtractor={(repo) => repo.id}
  renderItem={(repo) => <RepositoryCard repository={repo} />}
  emptyState={<p>No repositories found</p>}
/>
```

Esta biblioteca de componentes proporciona una base sólida y escalable para construir la interfaz de usuario de Hodei Artifacts, siguiendo las mejores prácticas de React moderno y asegurando consistencia, accesibilidad, performance y mantenibilidad a lo largo de toda la aplicación.