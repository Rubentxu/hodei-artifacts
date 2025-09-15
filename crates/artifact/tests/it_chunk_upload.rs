use bytes::Bytes;
use std::sync::Arc;
use tempfile::TempDir;

use artifact::features::upload_artifact::{
    adapter::LocalFsChunkedUploadStorage, error::UploadArtifactError, ports::ChunkedUploadStorage,
};

#[tokio::test]
async fn test_chunked_upload_storage_happy_path() -> Result<(), UploadArtifactError> {
    // Crear directorio temporal para los tests
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let storage = LocalFsChunkedUploadStorage::new(temp_dir.path().to_path_buf());
    let storage = Arc::new(storage);

    let upload_id = "test-upload-123";
    let chunk1_data = Bytes::from("This is chunk 1 data");
    let chunk2_data = Bytes::from("This is chunk 2 data");
    let chunk3_data = Bytes::from("This is chunk 3 data");

    // Guardar chunks
    storage
        .save_chunk(upload_id, 1, chunk1_data.clone())
        .await?;
    storage
        .save_chunk(upload_id, 2, chunk2_data.clone())
        .await?;
    storage
        .save_chunk(upload_id, 3, chunk3_data.clone())
        .await?;

    // Verificar que se han guardado correctamente
    let count = storage.get_received_chunks_count(upload_id).await?;
    assert_eq!(count, 3);

    // Obtener la lista de chunks recibidos
    let chunk_numbers = storage.get_received_chunk_numbers(upload_id).await?;
    assert_eq!(chunk_numbers, vec![1, 2, 3]);

    // Verificar que los datos de los chunks son correctos
    let upload_dir = temp_dir.path().join(upload_id);
    let chunk1_path = upload_dir.join("1");
    let chunk2_path = upload_dir.join("2");
    let chunk3_path = upload_dir.join("3");

    assert!(chunk1_path.exists());
    assert!(chunk2_path.exists());
    assert!(chunk3_path.exists());

    let chunk1_content = tokio::fs::read(&chunk1_path)
        .await
        .map_err(|e| UploadArtifactError::StorageError(format!("Failed to read chunk1: {}", e)))?;
    let chunk2_content = tokio::fs::read(&chunk2_path)
        .await
        .map_err(|e| UploadArtifactError::StorageError(format!("Failed to read chunk2: {}", e)))?;
    let chunk3_content = tokio::fs::read(&chunk3_path)
        .await
        .map_err(|e| UploadArtifactError::StorageError(format!("Failed to read chunk3: {}", e)))?;

    assert_eq!(chunk1_content, chunk1_data);
    assert_eq!(chunk2_content, chunk2_data);
    assert_eq!(chunk3_content, chunk3_data);

    // Ensamblar los chunks
    let (assembled_path, hash) = storage
        .assemble_chunks(upload_id, 3, "test-file.txt")
        .await?;

    // Verificar que el archivo ensamblado existe y tiene el contenido correcto
    assert!(assembled_path.exists());
    let assembled_content = tokio::fs::read(&assembled_path).await.map_err(|e| {
        UploadArtifactError::StorageError(format!("Failed to read assembled file: {}", e))
    })?;
    let expected_content = [chunk1_data, chunk2_data, chunk3_data].concat();
    assert_eq!(assembled_content, expected_content);

    // Verificar que el hash es correcto
    assert!(!hash.is_empty());
    assert_eq!(hash.len(), 64); // SHA-256 hash length

    // Limpiar
    storage.cleanup(upload_id).await?;
    assert!(!upload_dir.exists());

    Ok(())
}

#[tokio::test]
async fn test_chunked_upload_storage_partial_upload() -> Result<(), UploadArtifactError> {
    // Crear directorio temporal para los tests
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let storage = LocalFsChunkedUploadStorage::new(temp_dir.path().to_path_buf());
    let storage = Arc::new(storage);

    let upload_id = "partial-upload-456";
    let chunk1_data = Bytes::from("First chunk data");
    let chunk3_data = Bytes::from("Third chunk data (missing chunk 2)");

    // Guardar solo algunos chunks (simulando una subida parcial)
    storage
        .save_chunk(upload_id, 1, chunk1_data.clone())
        .await?;
    storage
        .save_chunk(upload_id, 3, chunk3_data.clone())
        .await?;

    // Verificar que solo se han guardado 2 chunks
    let count = storage.get_received_chunks_count(upload_id).await?;
    assert_eq!(count, 2);

    // Obtener la lista de chunks recibidos
    let chunk_numbers = storage.get_received_chunk_numbers(upload_id).await?;
    assert_eq!(chunk_numbers, vec![1, 3]); // Falta el chunk 2

    // Ensamblar los chunks (esto debería funcionar incluso con chunks faltantes)
    let (assembled_path, _hash) = storage
        .assemble_chunks(upload_id, 3, "partial-file.txt")
        .await?;

    // Verificar que el archivo ensamblado existe
    assert!(assembled_path.exists());

    // Limpiar
    storage.cleanup(upload_id).await?;

    Ok(())
}

#[tokio::test]
async fn test_chunked_upload_storage_empty_upload() -> Result<(), UploadArtifactError> {
    // Crear directorio temporal para los tests
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let storage = LocalFsChunkedUploadStorage::new(temp_dir.path().to_path_buf());
    let storage = Arc::new(storage);

    let upload_id = "empty-upload-789";

    // Obtener chunks recibidos de un upload que no existe
    let chunk_numbers = storage.get_received_chunk_numbers(upload_id).await?;
    assert_eq!(chunk_numbers, Vec::<usize>::new());

    // Obtener conteo de chunks de un upload que no existe
    let count = storage.get_received_chunks_count(upload_id).await?;
    assert_eq!(count, 0);

    Ok(())
}

#[tokio::test]
async fn test_chunked_upload_storage_chunk_overwrite() -> Result<(), UploadArtifactError> {
    // Crear directorio temporal para los tests
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let storage = LocalFsChunkedUploadStorage::new(temp_dir.path().to_path_buf());
    let storage = Arc::new(storage);

    let upload_id = "overwrite-upload-101";
    let chunk_data_v1 = Bytes::from("Original chunk data");
    let chunk_data_v2 = Bytes::from("Updated chunk data");

    // Guardar un chunk
    storage
        .save_chunk(upload_id, 1, chunk_data_v1.clone())
        .await?;

    // Guardar el mismo chunk con nuevos datos (sobreescribir)
    storage
        .save_chunk(upload_id, 1, chunk_data_v2.clone())
        .await?;

    // Verificar que solo hay un chunk
    let count = storage.get_received_chunks_count(upload_id).await?;
    assert_eq!(count, 1);

    // Obtener la lista de chunks recibidos
    let chunk_numbers = storage.get_received_chunk_numbers(upload_id).await?;
    assert_eq!(chunk_numbers, vec![1]);

    // Verificar que el contenido es el último que se guardó
    let upload_dir = temp_dir.path().join(upload_id);
    let chunk_path = upload_dir.join("1");
    let chunk_content = tokio::fs::read(&chunk_path)
        .await
        .map_err(|e| UploadArtifactError::StorageError(format!("Failed to read chunk: {}", e)))?;
    assert_eq!(chunk_content, chunk_data_v2);

    // Limpiar
    storage.cleanup(upload_id).await?;

    Ok(())
}
