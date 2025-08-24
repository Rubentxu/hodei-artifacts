//! Implementación de `ArtifactStorage` con S3 (INFRA-T3).

use async_trait::async_trait;
use aws_sdk_s3::{Client as S3Client, primitives::ByteStream};
use shared::{RepositoryId, ArtifactId};
use crate::{
    application::ports::ArtifactStorage,
    error::ArtifactError,
};

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
}
