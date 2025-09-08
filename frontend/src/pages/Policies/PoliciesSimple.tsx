/**
 * Página de gestión de políticas de seguridad (versión simplificada)
 * Utiliza la arquitectura Clean Code con servicios y hooks
 */

import React, { useState } from 'react';
import { Card } from '@/components/ui/Card';
import { PageHeader } from '@/components/layout/PageHeader';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { Badge } from '@/components/ui/Badge';
import { Shield, Plus, Eye, Edit, Trash2 } from 'lucide-react';

const PoliciesSimple = () => {
  const [showCreateForm, setShowCreateForm] = useState(false);
  const [newPolicy, setNewPolicy] = useState({
    name: '',
    description: '',
    policy: '',
    isActive: true
  });

  // Estado de políticas (simulado por ahora)
  const [policies] = useState([
    {
      id: '1',
      name: 'Usuario Estándar',
      description: 'Permite acceso de lectura a repositorios públicos',
      isActive: true,
      createdAt: '2024-01-15T10:30:00Z'
    },
    {
      id: '2',
      name: 'Desarrollador',
      description: 'Permite gestión completa de repositorios',
      isActive: true,
      createdAt: '2024-01-14T15:45:00Z'
    },
    {
      id: '3',
      name: 'Administrador',
      description: 'Acceso completo al sistema',
      isActive: false,
      createdAt: '2024-01-13T09:20:00Z'
    }
  ]);

  const handleCreatePolicy = () => {
    if (!newPolicy.name || !newPolicy.policy) {
      alert('Por favor complete todos los campos requeridos');
      return;
    }

    console.log('Creando política:', newPolicy);
    alert('Política creada exitosamente (simulado)');
    
    // Limpiar formulario
    setNewPolicy({
      name: '',
      description: '',
      policy: '',
      isActive: true
    });
    setShowCreateForm(false);
  };

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
        title="Security Policies"
        subtitle="Manage Cedar-based access control policies"
        actions={
          <Button onClick={() => setShowCreateForm(true)} size="sm">
            <Plus className="w-4 h-4 mr-2" />
            New Policy
          </Button>
        }
      />

      {/* Formulario de Creación */}
      {showCreateForm && (
        <Card className="p-6">
          <div className="space-y-6">
            <div className="flex items-center justify-between">
              <h3 className="text-lg font-semibold text-gray-900">Create New Policy</h3>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setShowCreateForm(false)}
              >
                Cancel
              </Button>
            </div>

            <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
              {/* Información Básica */}
              <div className="space-y-4">
                <div>
                  <label className="block text-sm font-medium text-gray-700 mb-2">
                    Policy Name *
                  </label>
                  <Input
                    value={newPolicy.name}
                    onChange={(e) => setNewPolicy(prev => ({ ...prev, name: e.target.value }))}
                    placeholder="E.g: Standard User"
                    className="w-full"
                  />
                </div>

                <div>
                  <label className="block text-sm font-medium text-gray-700 mb-2">
                    Description
                  </label>
                  <Input
                    value={newPolicy.description}
                    onChange={(e) => setNewPolicy(prev => ({ ...prev, description: e.target.value }))}
                    placeholder="Describe the purpose of this policy"
                    className="w-full"
                  />
                </div>

                <div className="flex items-center space-x-2">
                  <input
                    type="checkbox"
                    id="isActive"
                    checked={newPolicy.isActive}
                    onChange={(e) => setNewPolicy(prev => ({ ...prev, isActive: e.target.checked }))}
                    className="rounded border-gray-300"
                  />
                  <label htmlFor="isActive" className="text-sm font-medium text-gray-700">
                    Activate policy immediately
                  </label>
                </div>
              </div>

              {/* Política Cedar */}
              <div className="space-y-4">
                <div>
                  <label className="block text-sm font-medium text-gray-700 mb-2">
                    Cedar Policy *
                  </label>
                  <textarea
                    value={newPolicy.policy}
                    onChange={(e) => setNewPolicy(prev => ({ ...prev, policy: e.target.value }))}
                    placeholder="Write your Cedar policy here..."
                    className="w-full h-64 font-mono text-sm border border-gray-300 rounded-md p-3"
                    rows={12}
                  />
                </div>
                <p className="text-xs text-gray-500">
                  Use Cedar syntax to define permissions and restrictions.
                </p>
              </div>
            </div>

            <div className="flex justify-end gap-3">
              <Button
                variant="outline"
                onClick={() => setShowCreateForm(false)}
              >
                Cancel
              </Button>
              <Button
                onClick={handleCreatePolicy}
                disabled={!newPolicy.name || !newPolicy.policy}
              >
                Create Policy
              </Button>
            </div>
          </div>
        </Card>
      )}

      {/* Policy List */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
        {policies.map((policy) => (
          <Card key={policy.id} className="p-6 hover:shadow-lg transition-shadow">
            <div className="space-y-4">
              {/* Header */}
              <div className="flex items-start justify-between">
                <div className="flex items-center gap-3">
                  <div className="p-2 bg-blue-100 rounded-lg">
                    <Shield className="w-5 h-5 text-blue-600" />
                  </div>
                  <div>
                    <h4 className="font-semibold text-gray-900">{policy.name}</h4>
                    <Badge className={getStatusColor(policy.isActive)}>
                      {getStatusText(policy.isActive)}
                    </Badge>
                  </div>
                </div>
              </div>

              {/* Description */}
              <p className="text-sm text-gray-600">{policy.description}</p>

              {/* Creation Date */}
              <div className="text-xs text-gray-500">
                Created on {new Date(policy.createdAt).toLocaleDateString()}
              </div>

              {/* Actions */}
              <div className="flex gap-2 pt-4 border-t border-gray-200">
                <Button size="sm" variant="outline" className="flex-1">
                  <Eye className="w-4 h-4 mr-1" />
                  View
                </Button>
                <Button size="sm" variant="outline" className="flex-1">
                  <Edit className="w-4 h-4 mr-1" />
                  Edit
                </Button>
                <Button size="sm" variant="outline" className="flex-1">
                  <Trash2 className="w-4 h-4 mr-1" />
                  Delete
                </Button>
              </div>
            </div>
          </Card>
        ))}
      </div>

      {/* About Cedar */}
      <Card className="p-6">
        <div className="flex items-start gap-4">
          <div className="p-3 bg-blue-100 rounded-lg">
            <Shield className="w-6 h-6 text-blue-600" />
          </div>
          <div className="flex-1">
            <h3 className="text-lg font-semibold text-gray-900 mb-2">About Cedar</h3>
            <p className="text-sm text-gray-600 mb-4">
              Cedar is a policy language developed by AWS that allows defining authorization rules
              declaratively. Cedar policies determine who can do what on which resources.
            </p>
            <div className="flex gap-2">
              <Button size="sm" variant="outline">
                Documentation
              </Button>
              <Button size="sm" variant="outline">
                Examples
              </Button>
            </div>
          </div>
        </div>
      </Card>
    </div>
  );
};

export default PoliciesSimple;