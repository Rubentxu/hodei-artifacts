use async_trait::async_trait;
use redis::{AsyncCommands, Client};
use cedar_policy::{Decision, PolicyId};
use crate::application::ports::DecisionCache;
use crate::error::IamError;
use std::collections::HashSet;

pub struct RedisDecisionCache {
    client: Client,
}

impl RedisDecisionCache {
    pub fn new(redis_url: &str) -> Result<Self, IamError> {
        let client = Client::open(redis_url)
            .map_err(|e| IamError::InternalError(format!("Failed to connect to Redis: {}", e)))?;
        Ok(Self { client })
    }
}

#[async_trait]
impl DecisionCache for RedisDecisionCache {
    async fn get(&self, key: &str) -> Result<Option<(Decision, HashSet<PolicyId>)>, IamError> {
        let mut con = self.client.get_async_connection().await
            .map_err(|e| IamError::InternalError(format!("Failed to get Redis connection: {}", e)))?;

        let cached_data: Option<String> = con.get(key).await
            .map_err(|e| IamError::InternalError(format!("Failed to get from Redis: {}", e)))?;

        match cached_data {
            Some(json_str) => {
                let (decision, reason): (Decision, HashSet<PolicyId>) = serde_json::from_str(&json_str)
                    .map_err(|e| IamError::InternalError(format!("Failed to deserialize cached data: {}", e)))?;
                Ok(Some((decision, reason)))
            },
            None => Ok(None),
        }
    }

    async fn set(&self, key: &str, decision: Decision, reason: HashSet<PolicyId>, ttl_seconds: usize) -> Result<(), IamError> {
        let mut con = self.client.get_async_connection().await
            .map_err(|e| IamError::InternalError(format!("Failed to get Redis connection: {}", e)))?;

        let data_to_cache = (decision, reason);
        let json_str = serde_json::to_string(&data_to_cache)
            .map_err(|e| IamError::InternalError(format!("Failed to serialize data for caching: {}", e)))?;

        con.set_ex(key, json_str, ttl_seconds).await
            .map_err(|e| IamError::InternalError(format!("Failed to set in Redis: {}", e)))
    }
}
