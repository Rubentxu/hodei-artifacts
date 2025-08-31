use async_trait::async_trait;
use futures::stream::StreamExt;
use mongodb::{Client, Collection, IndexModel};
use mongodb::bson::doc;
use crate::application::ports::SearchIndex;
use crate::domain::model::ArtifactSearchDocument;
use crate::error::{SearchError, SearchResult};

const DB_NAME: &str = "hodei";
const COLLECTION_NAME: &str = "search_index";

pub struct MongoSearchIndex {
    collection: Collection<ArtifactSearchDocument>,
}

impl MongoSearchIndex {
    pub async fn new(client: &Client) -> SearchResult<Self> {
        let db = client.database(DB_NAME);
        let collection = db.collection(COLLECTION_NAME);

        // Create a text index on all string fields
        let index_model = IndexModel::builder()
            .keys(doc! { "$**": "text" })
            .build();
        collection.create_index(index_model).await.map_err(|e| {
            SearchError::IndexOperationFailed {
                operation: "create_text_index".to_string(),
                reason: e.to_string(),
            }
        })?;

        Ok(Self { collection })
    }
}

#[async_trait]
impl SearchIndex for MongoSearchIndex {
    async fn index(&self, document: &ArtifactSearchDocument) -> SearchResult<()> {
        self.collection
            .insert_one(document)
            .await
            .map_err(|e| SearchError::IndexOperationFailed {
                operation: "insert".to_string(),
                reason: e.to_string(),
            })?;
        Ok(())
    }

    async fn search(&self, query: &str, repository_filter: Option<String>) -> SearchResult<Vec<ArtifactSearchDocument>> {
        let mut filter = doc! { "$text": { "$search": query } };

        if let Some(repo_filter) = repository_filter {
            filter.insert("repository_id", repo_filter);
        }

        let mut cursor = self.collection.find(filter).await.map_err(|e| {
            SearchError::QueryFailed {
                message: e.to_string(),
            }
        })?;

        let mut results = Vec::new();
        while let Some(doc) = cursor.next().await {
            results.push(doc.map_err(|e| SearchError::QueryFailed {
                message: e.to_string(),
            })?);
        }

        Ok(results)
    }
}
