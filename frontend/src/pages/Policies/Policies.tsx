/**
 * Página de gestión de políticas de seguridad
 * Utiliza la arquitectura Clean Code con servicios y hooks
 */

import React, { useState, useCallback } from 'react';
import { Card } from '@/components/ui/Card';
import { PageHeader } from '@/components/layout/PageHeader';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { Textarea } from '@/components/ui/Input';
import { Badge } from '@/components/ui/Badge';
import { Checkbox } from '@/components/ui/Checkbox';
import { usePolicyService } from '@/shared/hooks';
import { Shield, Plus, Edit, Trash2, Eye, Download } from 'lucide-react';

const Policies = () => {
  const [showCreateForm, setShowCreateForm] = useState(false);
  const [newPolicy, setNewPolicy] = useState({
    name: '',
    description: '',
    policy: '',
    isActive: true,
  });

  const policyService = usePolicyService();

  // Estado de políticas (simulado por ahora)
  const [policies] = useState([
    {
      id: '1',
      name: 'Usuario Estándar',
      description: 'Permite acceso de lectura a repositorios públicos',
      isActive: true,
      createdAt: '2024-01-15T10:30:00Z',
    },
    {
      id: '2',
      name: 'Desarrollador',
      description: 'Permite gestión completa de repositorios',
      isActive: true,
      createdAt: '2024-01-14T15:45:00Z',
    },
    {
      id: '3',
      name: 'Administrador',
      description: 'Acceso completo al sistema',
      isActive: false,
      createdAt: '2024-01-13T09:20:00Z',
    },
  ]);

  const handleCreatePolicy = useCallback(async () => {
    if (!newPolicy.name || !newPolicy.policy) {
      alert('Por favor complete todos los campos requeridos');
      return;
    }

    try {
      // Aquí iría la llamada al servicio
      console.log('Creando política:', newPolicy);

      // Limpiar formulario
      setNewPolicy({
        name: '',
        description: '',
        policy: '',
        isActive: true,
      });
      setShowCreateForm(false);

      // Mostrar mensaje de éxito
      alert('Política creada exitosamente');
    } catch (error) {
      console.error('Error al crear política:', error);
      alert('Error al crear la política');
    }
  }, [newPolicy]);

  const handleGenerateTemplate = useCallback(
    (type: 'usuario' | 'repositorio' | 'admin') => {
      const template = policyService.generarPlantillaPolitica(type);
      setNewPolicy(prev => ({
        ...prev,
        policy: template,
      }));
    },
    [policyService]
  );

  const getStatusColor = (isActive: boolean) => {
    return isActive
      ? 'bg-green-100 text-green-800'
      : 'bg-gray-100 text-gray-800';
  };

  const getStatusText = (isActive: boolean) => {
    return isActive ? 'Activa' : 'Inactiva';
  };

  return (
    <div className="space-y-6">
      <PageHeader
        title="Políticas de Seguridad"
        subtitle="Gestiona las políticas de control de acceso basadas en Cedar"
        actions={
          <Button onClick={() => setShowCreateForm(true)} size="sm">
            <Plus className="w-4 h-4 mr-2" />
            Nueva Política
          </Button>
        }
      />

      {/* Formulario de Creación */}
      {showCreateForm && (
        <Card className="p-6">
          <div className="space-y-6">
            <div className="flex items-center justify-between">
              <h3 className="text-lg font-semibold text-gray-900">
                Crear Nueva Política
              </h3>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setShowCreateForm(false)}
              >
                Cancelar
              </Button>
            </div>

            <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
              {/* Información Básica */}
              <div className="space-y-4">
                <div>
                  <label className="block text-sm font-medium text-gray-700 mb-2">
                    Nombre de la Política *
                  </label>
                  <Input
                    value={newPolicy.name}
                    onChange={e =>
                      setNewPolicy(prev => ({ ...prev, name: e.target.value }))
                    }
                    placeholder="Ej: Usuario Estándar"
                    className="w-full"
                  />
                </div>

                <div>
                  <label className="block text-sm font-medium text-gray-700 mb-2">
                    Descripción
                  </label>
                  <Input
                    value={newPolicy.description}
                    onChange={e =>
                      setNewPolicy(prev => ({
                        ...prev,
                        description: e.target.value,
                      }))
                    }
                    placeholder="Describe el propósito de esta política"
                    className="w-full"
                  />
                </div>

                <div className="flex items-center space-x-2">
                  <Checkbox
                    id="isActive"
                    checked={newPolicy.isActive}
                    checked={newPolicy.isActive}
                    onCheckedChange={checked =>
                      setNewPolicy(prev => ({ ...prev, isActive: checked }))
                    }
                  />
                  <label
                    htmlFor="isActive"
                    className="text-sm font-medium text-gray-700"
                  >
                    Activar política inmediatamente
                  </label>
                </div>

                {/* Plantillas */}
                <div>
                  <label className="block text-sm font-medium text-gray-700 mb-2">
                    Plantillas Rápidas
                  </label>
                  <div className="flex gap-2">
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => handleGenerateTemplate('usuario')}
                    >
                      Usuario Estándar
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => handleGenerateTemplate('repositorio')}
                    >
                      Desarrollador
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => handleGenerateTemplate('admin')}
                    >
                      Administrador
                    </Button>
                  </div>
                </div>
              </div>

              {/* Política Cedar */}
              <div className="space-y-4">
                <div>
                  <label className="block text-sm font-medium text-gray-700 mb-2">
                    Política Cedar *
                  </label>
                  <Textarea
                    value={newPolicy.policy}
                    onChange={(e: React.ChangeEvent<HTMLTextAreaElement>) =>
                      setNewPolicy(prev => ({
                        ...prev,
                        policy: e.target.value,
                      }))
                    }
                    placeholder="Escribe tu política Cedar aquí..."
                    className="w-full h-64 font-mono text-sm"
                    rows={12}
                  />
                </div>
                <p className="text-xs text-gray-500">
                  Usa la sintaxis Cedar para definir permisos y restricciones.
                </p>
              </div>
            </div>

            <div className="flex justify-end gap-3">
              <Button
                variant="outline"
                onClick={() => setShowCreateForm(false)}
              >
                Cancelar
              </Button>
              <Button
                onClick={handleCreatePolicy}
                disabled={!newPolicy.name || !newPolicy.policy}
              >
                Crear Política
              </Button>
            </div>
          </div>
        </Card>
      )}

      {/* Lista de Políticas */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
        {policies.map(policy => (
          <Card
            key={policy.id}
            className="p-6 hover:shadow-lg transition-shadow"
          >
            <div className="space-y-4">
              {/* Encabezado */}
              <div className="flex items-start justify-between">
                <div className="flex items-center gap-3">
                  <div className="p-2 bg-blue-100 rounded-lg">
                    <Shield className="w-5 h-5 text-blue-600" />
                  </div>
                  <div>
                    <h4 className="font-semibold text-gray-900">
                      {policy.name}
                    </h4>
                    <Badge className={getStatusColor(policy.isActive)}>
                      {getStatusText(policy.isActive)}
                    </Badge>
                  </div>
                </div>
              </div>

              {/* Descripción */}
              <p className="text-sm text-gray-600">{policy.description}</p>

              {/* Fecha de Creación */}
              <div className="text-xs text-gray-500">
                Creada el {new Date(policy.createdAt).toLocaleDateString()}
              </div>

              {/* Acciones */}
              <div className="flex gap-2 pt-4 border-t border-gray-200">
                <Button size="sm" variant="outline" className="flex-1">
                  <Eye className="w-4 h-4 mr-1" />
                  Ver
                </Button>
                <Button size="sm" variant="outline" className="flex-1">
                  <Edit className="w-4 h-4 mr-1" />
                  Editar
                </Button>
                <Button size="sm" variant="outline" className="flex-1">
                  <Trash2 className="w-4 h-4 mr-1" />
                  Eliminar
                </Button>
              </div>
            </div>
          </Card>
        ))}
      </div>

      {/* Información de Cedar */}
      <Card className="p-6">
        <div className="flex items-start gap-4">
          <div className="p-3 bg-blue-100 rounded-lg">
            <Shield className="w-6 h-6 text-blue-600" />
          </div>
          <div className="flex-1">
            <h3 className="text-lg font-semibold text-gray-900 mb-2">
              Acerca de Cedar
            </h3>
            <p className="text-sm text-gray-600 mb-4">
              Cedar es un lenguaje de política desarrollado por AWS que permite
              definir reglas de autorización de manera declarativa. Las
              políticas Cedar determinan quién puede hacer qué en qué recursos.
            </p>
            <div className="flex gap-2">
              <Button size="sm" variant="outline">
                <Download className="w-4 h-4 mr-1" />
                Documentación
              </Button>
              <Button size="sm" variant="outline">
                Ejemplos
              </Button>
            </div>
          </div>
        </div>
      </Card>
    </div>
  );
};

export default Policies;
