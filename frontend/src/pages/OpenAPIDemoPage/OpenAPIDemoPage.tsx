/**
 * Página de demostración del API OpenAPI Contract First
 * Muestra la funcionalidad completa implementada siguiendo el patrón Contract First
 */

import React from 'react';
import { OpenAPIDemo, OpenAPIDemoCompact } from '@/features/openapi-demo/OpenAPIDemo';
import { Card } from '@/components/ui/Card';
import { Badge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { useState } from 'react';

/**
 * Página principal de demostración del API OpenAPI Contract First
 * Implementa el patrón Contract First con respuestas mockeadas basadas en el contrato OpenAPI
 */
export function OpenAPIDemoPage(): React.ReactElement {
  const [viewMode, setViewMode] = useState<'full' | 'compact'>('full');

  return (
    <div className="min-h-screen bg-gray-50">
      {/* Header de la página */}
      <div className="bg-white shadow-sm border-b">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="py-6">
            <div className="flex items-center justify-between">
              <div>
                <h1 className="text-3xl font-bold text-gray-900">
                  OpenAPI Contract First Demo
                </h1>
                <p className="mt-2 text-gray-600">
                  Complete implementation following the OpenAPI 3.0.3 contract specification
                </p>
              </div>
              <div className="flex items-center space-x-4">
                <Badge variant="success" size="lg">
                  Contract First Pattern
                </Badge>
                <div className="flex space-x-2">
                  <Button
                    onClick={() => setViewMode('full')}
                    variant={viewMode === 'full' ? 'primary' : 'outline'}
                    size="sm"
                  >
                    Full Demo
                  </Button>
                  <Button
                    onClick={() => setViewMode('compact')}
                    variant={viewMode === 'compact' ? 'primary' : 'outline'}
                    size="sm"
                  >
                    Compact View
                  </Button>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* Contenido principal */}
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8">
        {viewMode === 'full' ? (
          <OpenAPIDemo />
        ) : (
          <OpenAPIDemoCompact />
        )}
      </div>

      {/* Footer informativo */}
      <div className="bg-white border-t mt-12">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8">
          <div className="grid md:grid-cols-3 gap-8">
            <Card className="p-6">
              <h3 className="text-lg font-semibold text-gray-900 mb-3">
                🎯 Contract First Benefits
              </h3>
              <ul className="space-y-2 text-sm text-gray-600">
                <li>✅ Zero breaking changes when switching to real backend</li>
                <li>✅ Type safety with generated TypeScript types</li>
                <li>✅ Consistent API interface across frontend</li>
                <li>✅ Easy testing with mock responses</li>
              </ul>
            </Card>

            <Card className="p-6">
              <h3 className="text-lg font-semibold text-gray-900 mb-3">
                🔧 Implementation Details
              </h3>
              <ul className="space-y-2 text-sm text-gray-600">
                <li>📋 OpenAPI 3.0.3 contract as source of truth</li>
                <li>🔄 React Query for state management</li>
                <li>🎯 Custom hooks for each API domain</li>
                <li>📊 Mock responses following exact schemas</li>
              </ul>
            </Card>

            <Card className="p-6">
              <h3 className="text-lg font-semibold text-gray-900 mb-3">
                🚀 Next Steps
              </h3>
              <ul className="space-y-2 text-sm text-gray-600">
                <li>🔌 Replace mock client with real HTTP client</li>
                <li>🔄 Update base URL configuration</li>
                <li>✅ All components work without changes</li>
                <li>🎯 Zero refactoring needed</li>
              </ul>
            </Card>
          </div>

          <div className="mt-8 text-center">
            <Card className="p-6 bg-blue-50 border-blue-200">
              <h3 className="text-lg font-semibold text-blue-900 mb-2">
                🎉 Contract First Implementation Complete!
              </h3>
              <p className="text-blue-800 text-sm">
                The frontend is now ready to connect to the real Rust backend. 
                When the backend is available, simply replace the mock client implementation 
                and all functionality will work seamlessly.
              </p>
            </Card>
          </div>
        </div>
      </div>
    </div>
  );
}

/**
 * Componente de información técnica sobre el patrón Contract First
 */
export function ContractFirstInfo(): React.ReactElement {
  return (
    <Card className="p-6 bg-gradient-to-r from-blue-50 to-purple-50 border-blue-200">
      <div className="text-center">
        <h2 className="text-2xl font-bold text-gray-900 mb-4">
          OpenAPI Contract First Pattern
        </h2>
        <p className="text-gray-700 mb-6 max-w-3xl mx-auto">
          This implementation follows the Contract First pattern, where the OpenAPI specification 
          serves as the single source of truth for the API contract between frontend and backend.
        </p>
        
        <div className="grid md:grid-cols-2 gap-6 text-left">
          <div>
            <h3 className="font-semibold text-gray-900 mb-3">📋 Contract Components</h3>
            <ul className="space-y-2 text-sm text-gray-600">
              <li>• <code>openapi.yaml</code> - Main contract specification</li>
              <li>• <code>paths/</code> - Individual endpoint definitions</li>
              <li>• <code>components/schemas/</code> - Data models and types</li>
              <li>• <code>components/responses.yaml</code> - Standardized responses</li>
            </ul>
          </div>
          
          <div>
            <h3 className="font-semibold text-gray-900 mb-3">🔄 Implementation Flow</h3>
            <ul className="space-y-2 text-sm text-gray-600">
              <li>1. Define contract in OpenAPI YAML</li>
              <li>2. Generate TypeScript types from schemas</li>
              <li>3. Implement mock client following contract</li>
              <li>4. Create custom hooks for React components</li>
              <li>5. Replace mock with real client when ready</li>
            </ul>
          </div>
        </div>
      </div>
    </Card>
  );
}

/**
 * Tabla de comparación entre Mock y Real Implementation
 */
export function ImplementationComparison(): React.ReactElement {
  return (
    <Card className="p-6">
      <h2 className="text-xl font-semibold text-gray-900 mb-4">
        Mock vs Real Implementation Comparison
      </h2>
      
      <div className="overflow-x-auto">
        <table className="min-w-full divide-y divide-gray-200">
          <thead className="bg-gray-50">
            <tr>
              <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                Aspect
              </th>
              <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                Mock Implementation
              </th>
              <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                Real Implementation
              </th>
            </tr>
          </thead>
          <tbody className="bg-white divide-y divide-gray-200">
            <tr>
              <td className="px-6 py-4 whitespace-nowrap text-sm font-medium text-gray-900">
                Data Source
              </td>
              <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                Mock data following OpenAPI schemas
              </td>
              <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                Real data from Rust backend
              </td>
            </tr>
            <tr>
              <td className="px-6 py-4 whitespace-nowrap text-sm font-medium text-gray-900">
                Response Time
              </td>
              <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                Simulated delays (200-600ms)
              </td>
              <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                Real network latency
              </td>
            </tr>
            <tr>
              <td className="px-6 py-4 whitespace-nowrap text-sm font-medium text-gray-900">
                Error Handling
              </td>
              <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                Controlled error scenarios
              </td>
              <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                                Real error conditions
              </td>
            </tr>
            <tr>
              <td className="px-6 py-4 whitespace-nowrap text-sm font-medium text-gray-900">
                Authentication
              </td>
              <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                Mock auth with hardcoded credentials
              </td>
              <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                Real JWT tokens and sessions
              </td>
            </tr>
            <tr>
              <td className="px-6 py-4 whitespace-nowrap text-sm font-medium text-gray-900">
                File Uploads
              </td>
              <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                Simulated file processing
              </td>
              <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                Real file storage and processing
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </Card>
  );
}