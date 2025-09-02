//! Implementación de `ArtifactStorage` con S3 (INFRA-T3).

use async_trait::async_trait;
use aws_sdk_s3::{Client as S3Client, primitives::ByteStream};
use shared::{RepositoryId, ArtifactId};
use crate::{
    application::ports::ArtifactStorage,
    error::ArtifactError,
};

#[derive(Clone)]
pub struct S3ArtifactStorage {
    s3_client: S3Client,
    bucket_name: String,
}

impl S3ArtifactStorage {
    pub fn new(s3_client: S3Client, bucket_name: impl Into<String>) -> Self {
        Self {
            s3_client,
            bucket_name: bucket_name.into(),
        }
    }

    fn build_object_key(&self, repository_id: &RepositoryId, artifact_id: &ArtifactId) -> String {
        format!("{}/{}", repository_id.0, artifact_id.0)
    }
}

#[async_trait]
impl ArtifactStorage for S3ArtifactStorage {
    async fn put_object(
        &self,
        repository_id: &RepositoryId,
        artifact_id: &ArtifactId,
        bytes: &[u8],
    ) -> Result<(), ArtifactError> {
        let key = self.build_object_key(repository_id, artifact_id);
        let body = ByteStream::from(bytes.to_vec());

        self.s3_client
            .put_object()
            .bucket(&self.bucket_name)
            .key(key)
            .body(body)
            .send()
            .await
            .map_err(|e| ArtifactError::Storage(format!("S3 put_object: {e}")))?;

        Ok(())
    }

    async fn get_object_stream(
        &self,
        repository_id: &RepositoryId,
        artifact_id: &ArtifactId,
    ) -> Result<Vec<u8>, ArtifactError> {
        let key = self.build_object_key(repository_id, artifact_id);

        let response = self.s3_client
            .get_object()
            .bucket(&self.bucket_name)
            .key(key)
            .send()
            .await
            .map_err(|e| ArtifactError::Storage(format!("S3 get_object: {e}")))?;

        let bytes = response.body
            .collect()
            .await
            .map_err(|e| ArtifactError::Storage(format!("S3 read body: {e}")))?
            .into_bytes()
            .to_vec();

        Ok(bytes)
    }

    async fn get_presigned_download_url(
        &self,
        repository_id: &RepositoryId,
        artifact_id: &ArtifactId,
        expires_in_secs: u64,
    ) -> Result<String, ArtifactError> {
        let key = self.build_object_key(repository_id, artifact_id);

        let presigned_req = self.s3_client
            .get_object()
            .bucket(&self.bucket_name)
            .key(key)
            .presigned(
                aws_sdk_s3::presigning::PresigningConfig::expires_in(
                    std::time::Duration::from_secs(expires_in_secs)
                ).map_err(|e| ArtifactError::Storage(format!("Presigning config: {e}")))?
            )
            .await
            .map_err(|e| ArtifactError::Storage(format!("S3 presign: {e}")))?;

        Ok(presigned_req.uri().to_string())
    }
}
