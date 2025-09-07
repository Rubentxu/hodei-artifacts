# 4. Matriz de Dependencias Basada en Eventos

| Evento Disparador | Acciones Resultantes | Épicas Involucradas |
|-------------------|---------------------|-------------------|
| ArtifactUploaded | Escaneo seguridad, Generación SBOM, Indexación | E1 → E6 → E3 |
| VulnerabilityDetected | Cuarentena artefacto, Notificación seguridad | E6 → E2 → E4 |
| SearchQueryExecuted | Analytics uso, Personalización resultados | E3 → E7 |
| AccessDecisionMade | Audit trail, Métricas desempeño | E4 → E7 |
