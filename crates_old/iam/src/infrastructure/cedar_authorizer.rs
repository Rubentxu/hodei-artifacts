use std::sync::Arc;
use cedar_policy::{Authorizer, PolicySet, Response, Entities, Request, EntityUid, Context};
use crate::error::IamError;
use async_trait::async_trait;
use serde::Serialize;
use serde_with::{serde_as, DisplayFromStr};
use crate::application::ports::{Authorization, DecisionCache};

#[serde_as]
#[derive(Serialize)]
struct SerializableRequest<'a> {
    #[serde_as(as = "Option<DisplayFromStr>")]
    principal: Option<&'a EntityUid>,
    #[serde_as(as = "Option<DisplayFromStr>")]
    action: Option<&'a EntityUid>,
    #[serde_as(as = "Option<DisplayFromStr>")]
    resource: Option<&'a EntityUid>,
    #[serde_as(as = "Option<DisplayFromStr>")]
    context: Option<&'a Context>,
}

pub struct CedarAuthorizer {
    authorizer: Authorizer,
    policies: PolicySet,
    cache: Arc<dyn DecisionCache>,
}

impl CedarAuthorizer {
    pub fn new(policies: PolicySet, cache: Arc<dyn DecisionCache>) -> Self {
        Self {
            authorizer: Authorizer::new(),
            policies,
            cache,
        }
    }
}

#[async_trait]
impl Authorization for CedarAuthorizer {
    async fn is_authorized(&self, request: Request) -> Result<Response, IamError> {
        let serializable_request = SerializableRequest {
            principal: request.principal(),
            action: request.action(),
            resource: request.resource(),
            context: request.context(),
        };
        let cache_key = serde_json::to_string(&serializable_request)
            .map_err(|e| IamError::InternalError(format!("Failed to serialize request for cache key: {}", e)))?;

        if let Some((cached_decision, cached_reason)) = self.cache.get(&cache_key).await? {
            let response = Response::new(cached_decision, cached_reason, vec![]);
            return Ok(response);
        }

        let response = self.authorizer.is_authorized(&request, &self.policies, &Entities::empty());

        let reason = response.diagnostics().reason().cloned().collect();
        self.cache.set(&cache_key, response.decision(), reason, 3600).await?;

        Ok(response)
    }
}
