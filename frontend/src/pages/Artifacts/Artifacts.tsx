/**
 * Página de gestión de artefactos
 * Utiliza la arquitectura Clean Code con servicios y hooks
 */

import React, { useState, useCallback } from 'react';
import { Card } from '@/components/ui/Card';
import { PageHeader } from '@/components/layout/PageHeader';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { Badge } from '@/components/ui/Badge';
import { 
  useUploadArtifact,
  useValidateArtifact,
  useAnalyzePackageType,
  useGenerateArtifactMetadata
} from '@/shared/hooks/artifacts';
import { Upload, Package, CheckCircle, XCircle, FileText, Download } from 'lucide-react';

const Artifacts = () => {
  const [selectedFile, setSelectedFile] = useState<File | null>(null);
  const [repositoryId, setRepositoryId] = useState('');
  const [metadata, setMetadata] = useState<Record<string, any>>({});

  // Hooks de mutación
  const uploadArtifact = useUploadArtifact();
  const validateArtifact = useValidateArtifact();
  const analyzePackageType = useAnalyzePackageType();
  const generateMetadata = useGenerateArtifactMetadata();

  // Estado de validación
  const [validationResult, setValidationResult] = useState<{
    valid: boolean;
    errors: string[];
  } | null>(null);
  const [packageType, setPackageType] = useState<'maven' | 'npm' | 'pypi' | 'unknown'>('unknown');

  const handleFileSelect = useCallback(async (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (!file) return;

    setSelectedFile(file);
    
    // Validar archivo
    const validation = await validateArtifact.mutateAsync(file);
    setValidationResult(validation);
    
    // Analizar tipo de paquete
    const type = await analyzePackageType.mutateAsync(file.name);
    setPackageType(type);
    
    // Generar metadatos
    const generatedMetadata = await generateMetadata.mutateAsync({
      file,
      type: type === 'unknown' ? undefined : type
    });
    setMetadata(generatedMetadata);
  }, [validateArtifact, analyzePackageType, generateMetadata]);

  const handleUpload = useCallback(async () => {
    if (!selectedFile || !validationResult?.valid) return;

    try {
      const uploadBody = {
        file: selectedFile,
        metadata: JSON.stringify(metadata)
      };

      await uploadArtifact.mutateAsync(uploadBody);
      
      // Clear form after success
      setSelectedFile(null);
      setValidationResult(null);
      setPackageType('unknown');
      setMetadata({});
    } catch (error) {
      console.error('Error uploading artifact:', error);
    }
  }, [selectedFile, validationResult, metadata, uploadArtifact]);

  const getPackageTypeIcon = (type: string) => {
    switch (type) {
      case 'maven':
        return <Package className="w-4 h-4 text-blue-500" />;
      case 'npm':
        return <Package className="w-4 h-4 text-green-500" />;
      case 'pypi':
        return <Package className="w-4 h-4 text-yellow-500" />;
      default:
        return <Package className="w-4 h-4 text-gray-500" />;
    }
  };

  const getPackageTypeColor = (type: string) => {
    switch (type) {
      case 'maven':
        return 'bg-blue-100 text-blue-800';
      case 'npm':
        return 'bg-green-100 text-green-800';
      case 'pypi':
        return 'bg-yellow-100 text-yellow-800';
      default:
        return 'bg-gray-100 text-gray-800';
    }
  };

  const isUploadDisabled = !selectedFile || !validationResult?.valid || uploadArtifact.isPending;

  return (
    <div className="space-y-6">
      <PageHeader
        title="Gestión de Artefactos"
        subtitle="Sube y gestiona tus artefactos de software"
        actions={
          <div className="flex gap-2">
            <Button variant="outline" size="sm">
              <Download className="w-4 h-4 mr-2" />
              Descargar Plantilla
            </Button>
            <Button size="sm">
              <Upload className="w-4 h-4 mr-2" />
              Subir Artefacto
            </Button>
          </div>
        }
      />

      {/* Formulario de Subida */}
      <Card className="p-6">
        <div className="space-y-6">
          <div>
            <h3 className="text-lg font-semibold text-gray-900 mb-4">Subir Nuevo Artefacto</h3>
            
            {/* File Selector */}
            <div className="mb-4">
              <label className="block text-sm font-medium text-gray-700 mb-2">
                Select File
              </label>
              <div className="border-2 border-dashed border-gray-300 rounded-lg p-6 text-center hover:border-gray-400 transition-colors">
                <Input
                  type="file"
                  onChange={handleFileSelect}
                  accept=".jar,.war,.ear,.pom,.tgz,.tar.gz,.whl,.egg,.zip"
                  className="hidden"
                  id="artifact-upload"
                />
                <label htmlFor="artifact-upload" className="cursor-pointer">
                  <Upload className="w-12 h-12 text-gray-400 mx-auto mb-4" />
                  <p className="text-sm text-gray-600">
                    {selectedFile ? selectedFile.name : 'Click to select a file'}
                  </p>
                  <p className="text-xs text-gray-500 mt-1">
                    Allowed formats: JAR, WAR, EAR, POM, TGZ, WHL, EGG, ZIP
                  </p>
                </label>
              </div>
            </div>

            {/* Repository ID */}
            <div className="mb-4">
              <label className="block text-sm font-medium text-gray-700 mb-2">
                Repository ID (Optional)
              </label>
              <Input
                type="text"
                value={repositoryId}
                onChange={(e) => setRepositoryId(e.target.value)}
                placeholder="Enter destination repository ID"
                className="w-full"
              />
            </div>

            {/* Validation and Metadata */}
            {selectedFile && (
              <div className="space-y-4">
                {/* Validation Result */}
                {validationResult && (
                  <div className={`p-4 rounded-lg flex items-center gap-3 ${
                    validationResult.valid
                      ? 'bg-green-50 border border-green-200'
                      : 'bg-red-50 border border-red-200'
                  }`}>
                    {validationResult.valid ? (
                      <CheckCircle className="w-5 h-5 text-green-600" />
                    ) : (
                      <XCircle className="w-5 h-5 text-red-600" />
                    )}
                    <div>
                      <p className={`font-medium ${
                        validationResult.valid ? 'text-green-800' : 'text-red-800'
                      }`}>
                        {validationResult.valid ? 'Valid File' : 'Invalid File'}
                      </p>
                      {validationResult.errors.length > 0 && (
                        <ul className="text-sm text-red-700 mt-1">
                          {validationResult.errors.map((error, index) => (
                            <li key={index}>• {error}</li>
                          ))}
                        </ul>
                      )}
                    </div>
                  </div>
                )}

                {/* Package Type */}
                {packageType !== 'unknown' && (
                  <div className="flex items-center gap-3 p-3 bg-blue-50 border border-blue-200 rounded-lg">
                    {getPackageTypeIcon(packageType)}
                    <div>
                      <p className="font-medium text-blue-800">Detected Package Type</p>
                      <Badge className={getPackageTypeColor(packageType)}>
                        {packageType.toUpperCase()}
                      </Badge>
                    </div>
                  </div>
                )}

                {/* Generated Metadata */}
                {Object.keys(metadata).length > 0 && (
                  <div className="p-4 bg-gray-50 border border-gray-200 rounded-lg">
                    <div className="flex items-center gap-2 mb-2">
                      <FileText className="w-4 h-4 text-gray-600" />
                      <p className="font-medium text-gray-800">Artifact Metadata</p>
                    </div>
                    <div className="text-sm text-gray-600 space-y-1">
                      <p><strong>Name:</strong> {metadata.filename}</p>
                      <p><strong>Size:</strong> {(metadata.size / 1024 / 1024).toFixed(2)} MB</p>
                      <p><strong>Type:</strong> {metadata.packageType}</p>
                      <p><strong>Date:</strong> {new Date(metadata.uploadedAt).toLocaleString()}</p>
                    </div>
                  </div>
                )}
              </div>
            )}

            {/* Upload Button */}
            <Button
              onClick={handleUpload}
              disabled={isUploadDisabled}
              className="w-full"
            >
              {uploadArtifact.isPending ? (
                <>
                  <div className="animate-spin rounded-full h-4 w-4 border-b-2 border-white mr-2"></div>
                  Uploading...
                </>
              ) : (
                <>
                  <Upload className="w-4 h-4 mr-2" />
                  Upload Artifact
                </>
              )}
            </Button>
          </div>
        </div>
      </Card>

      {/* Lista de Artefactos Recientes */}
      <Card className="p-6">
        <div className="flex items-center justify-between mb-4">
          <h3 className="text-lg font-semibold text-gray-900">Artefactos Recientes</h3>
          <Button variant="outline" size="sm">
            Ver Todos
          </Button>
        </div>
        
        <div className="text-center py-8 text-gray-500">
          <Package className="w-12 h-12 mx-auto mb-4 text-gray-300" />
          <p>No hay artefactos subidos aún</p>
          <p className="text-sm mt-1">Sube tu primer artefacto usando el formulario de arriba</p>
        </div>
      </Card>
    </div>
  );
};

export default Artifacts;