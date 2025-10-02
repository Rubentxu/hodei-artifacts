// crates/shared/src/security/cedar_integration.rs

use crate::security::HodeiResource;
use cedar_policy::{Entity, EntityUid, RestrictedExpression};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CedarConversionError {
    #[error("Failed to convert to Cedar entity: {0}")]
    ConversionError(String),
}

/// Convierte cualquier entidad Hodei a una entidad Cedar para evaluación de políticas
pub fn to_cedar_entity<R, IdType, AttrType>(resource: &R) -> Result<Entity, CedarConversionError>
where
    R: HodeiResource<IdType, AttrType>,
    IdType: Into<EntityUid>,
    AttrType: Into<RestrictedExpression>,
{
    let uid = resource.resource_id().into();
    let attrs = resource
        .resource_attributes()
        .into_iter()
        .map(|(k, v)| (k, v.into()))
        .collect();
    let parents = resource
        .resource_parents()
        .into_iter()
        .map(|p| p.into())
        .collect();

    Entity::new(uid, attrs, parents)
        .map_err(|e| CedarConversionError::ConversionError(e.to_string()))
}

/// Conversor para transformar múltiples entidades Hodei al formato Cedar
pub struct CedarEntityConverter;

impl CedarEntityConverter {
    /// Convierte un conjunto de entidades para evaluación de políticas Cedar DSL
    pub fn convert_entities<R, IdType, AttrType>(
        entities: &[R],
    ) -> Result<Vec<Entity>, CedarConversionError>
    where
        R: HodeiResource<IdType, AttrType>,
        IdType: Into<EntityUid>,
        AttrType: Into<RestrictedExpression>,
    {
        entities.iter().map(to_cedar_entity).collect()
    }
}
