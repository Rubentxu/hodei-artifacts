# Épica E3: 🔍 Search & Discovery Engine
**Objetivo**: Búsqueda avanzada y descubrimiento inteligente  
**Valor de Negocio**: Usabilidad y experiencia desarrollador  
**Complejidad**: ⭐⭐⭐ (Alta)  
**Flujo Event Storming**: Flujo 9 (Re-evaluación Seguridad Proactiva) + Integración con todos los flujos  
**Eventos Clave**: SearchQueryExecuted, SearchResultClicked, ArtifactIndexed, PopularSearchDetected, SlowSearchDetected, SearchIndexUpdated

## Features Principales con Contexto de Eventos (22 features)
| Feature ID | Nombre | Descripción | Eventos Relacionados | Use Cases | Prioridad | Estimación |
|------------|--------|-------------|---------------------|-----------|-----------|------------|
| E3.F01 | **Basic Search Engine** | Búsqueda texto nombre/versión | SearchQueryExecuted, SearchResultsReturned | Buscar por Nombre, Buscar por Versión | P0 | 8 pts |
| E3.F02 | **Advanced Query Parser** | Sintaxis query avanzada | SearchQueryParsed, SearchQueryValidated | Búsqueda por Propiedad/Metadato | P1 | 8 pts |
| E3.F03 | **Full-Text Search** | Búsqueda contenido/metadata | SearchQueryExecuted, FullTextSearchPerformed | Buscar por Nombre, Navegar por Repositorio | P1 | 5 pts |
| E3.F04 | **Faceted Search** | Filtros por categorías | SearchFacetsApplied, SearchResultsFiltered | Búsqueda por Licencia, Búsqueda por Vulnerabilidad | P1 | 5 pts |
| E3.F05 | **Search Suggestions** | Auto-completado inteligente | SearchSuggestionsGenerated, SearchSuggestionSelected | Buscar por Nombre | P2 | 5 pts |
| E3.F06 | **Search Analytics** | Métricas queries populares | SearchAnalyticsCollected, PopularSearchDetected | Generar Informe de Uso de Artefactos | P2 | 3 pts |
| E3.F07 | **Saved Searches** | Búsquedas guardadas por usuario | SearchSaved, SearchLoaded | Buscar por Nombre | P2 | 3 pts |
| E3.F08 | **Search API Rate Limiting** | Protección abuse | SearchRateLimited, RateLimitExceeded | Buscar por Nombre | P1 | 3 pts |
| E3.F09 | **Search Indexing Pipeline** | Pipeline indexación tiempo real | ArtifactIndexed, SearchIndexUpdated, IndexingPipelineCompleted | Buscar por Nombre | P0 | 13 pts |
| E3.F10 | **Search Result Ranking** | Algoritmo relevancia personalizable | SearchResultsRanked, RankingAlgorithmApplied | Buscar por Nombre | P1 | 8 pts |
| E3.F11 | **Search by Hash** | Búsqueda por SHA-256 exacto | SearchByHashExecuted, HashSearchCompleted | Buscar por Checksum | P1 | 3 pts |
| E3.F12 | **Search by License** | Filtros por tipo licencia | SearchByLicense, LicenseFilterApplied | Búsqueda por Licencia | P2 | 3 pts |
| E3.F13 | **Search by Vulnerability** | Filtros por vulnerabilidades | SearchByVulnerability, VulnerabilityFilterApplied | Búsqueda por Vulnerabilidad | P1 | 5 pts |
| E3.F14 | **Search Export** | Exportar resultados CSV/JSON | SearchResultsExported, ExportFormatGenerated | Exportar Logs de Eventos | P2 | 3 pts |
| E3.F15 | **Search Personalization** | Resultados personalizados | SearchPersonalized, UserPreferencesApplied | Buscar por Nombre | P2 | 8 pts |
| E3.F16 | **Search Performance Optimization** | Índices optimizados | SearchPerformanceOptimized, IndexOptimized | Buscar por Nombre | P1 | 5 pts |
| E3.F17 | **Search Federation** | Búsqueda múltiples repositorios | SearchFederated, FederationResultsMerged | Buscar por Nombre | P2 | 8 pts |
| E3.F18 | **Search Alerts** | Notificaciones nuevos artefactos | SearchAlertCreated, SearchAlertTriggered | Configurar Alertas Personalizadas | P2 | 5 pts |
| E3.F19 | **Search API Documentation** | Docs interactiva API búsqueda | SearchAPIAccessed, DocumentationViewed | Buscar por Nombre | P2 | 3 pts |
| E3.F20 | **Search Monitoring** | Monitoreo performance búsquedas | SearchPerformanceMonitored, SlowSearchDetected | Generar Informe de Uso de Artefactos | P1 | 3 pts |
| E3.F21 | **Search Cache Layer** | Cache consultas frecuentes | SearchCacheHit, SearchCacheMiss | Buscar por Nombre | P1 | 5 pts |
| E3.F22 | **Search ML Recommendations** | Recomendaciones ML básicas | MLRecommendationsGenerated, RecommendationClicked | Buscar por Nombre | P3 | 13 pts |

## Integración con Flujo 9 (Re-evaluación Seguridad Proactiva)
- **Consulta Compleja**: "Encontrar todos los PackageVersion que tienen un componente SBOM específico con versión vulnerable"
- **Evento Resultante**: `PotencialesArtefactosAfectadosIdentificados`
- **Acción**: Disparar re-evaluación de seguridad para artefactos afectados

## Use Cases Avanzados Integrados
- "Búsqueda por Componente Interno (SBOM)": Hallar artefactos que contienen dependencias específicas
- "Búsqueda por Propiedad/Metadato": Filtrar basado en etiquetas personalizadas
- "Navegar por Repositorio": Exploración visual del contenido
- "Listar Versiones de un Paquete": Vista completa de todas las versiones

## Integraciones Cruzadas
- **E1 (Upload)**: Indexación automática post-upload (política del sistema)
- **E6 (Security)**: Búsqueda por vulnerabilidades y estado de seguridad
- **E7 (Analytics)**: Métricas de uso de búsquedas y patrones de consulta
- **Todos los flujos**: Búsqueda unificada a través de todos los contextos

## Métricas de Éxito Extendidas
- **Latencia p99**: <50ms para consultas de búsqueda
- **Precisión**: >95% de resultados relevantes
- **Indexación Tiempo Real**: <1s desde upload hasta disponibilidad en búsqueda
- **Disponibilidad**: 99.9% uptime para servicio de búsqueda
- **Throughput**: >1000 consultas concurrentes


## Investigación Técnica Detallada

### Estado Actual Integrado
- **Tantivy 0.25.0**: Ya integrado para búsqueda básica
- **Integración Kafka**: Indexación event-driven implementada
- **APIs de búsqueda básicas**: Endpoints REST definidos en OpenAPI
- **Arquitectura Vertical Slice**: Features de búsqueda bien estructurados

### Dependencias Recomendadas
```toml
# Añadir a crates/search/Cargo.toml
[dependencies]
# Mejoras core de búsqueda
fst = "0.4.7"                    # Transductores de estado finito para autocompletado
fuzzy-aho-corasick = "0.3.6"     # Búsqueda difusa con tolerancia a errores
moka = "0.12.10"                 # Caching de alto rendimiento
rayon = "1.8.1"                  # Procesamiento paralelo para indexación

# Analytics & monitoreo  
histogram = "0.7.0"              # Histogramas de performance
prometheus = "0.13.3"            # Métricas de búsqueda (ya en workspace)

[dev-dependencies]
criterion = "0.5.1"              # Benchmarking
```

### Algoritmos Clave de Implementación

**1. Pipeline de Indexación en Tiempo Real**
```rust
// Indexación event-driven desde Kafka
async fn process_artifact_uploaded(event: ArtifactUploaded) -> Result<()> {
    let artifact = fetch_complete_metadata(&event.artifact_id).await?;
    
    let search_doc = ArtifactSearchDocument {
        artifact_id: artifact.id,
        name: artifact.name,
        description: artifact.description,
        tags: extract_tags(&artifact.metadata),
        package_type: detect_package_type(&artifact.name),
        license: extract_license(&artifact.metadata),
        downloads: 0,
        last_modified: artifact.upload_date,
        indexed_at: Utc::now(),
        relevance_score: 1.0,
    };
    
    retry_with_backoff(|| search_index.index(&search_doc)).await?;
    Ok(())
}
```

**2. Autocompletado basado en FST**
```rust
// Transductor de estado finito para sugerencias
struct AutocompleteEngine {
    fst: fst::Map<Vec<u8>>,
    suggestions: HashMap<String, Vec<Suggestion>>,
}

impl AutocompleteEngine {
    fn suggest(&self, prefix: &str, limit: usize) -> Vec<Suggestion> {
        let mut results = Vec::new();
        let mut stream = self.fst.search(fst::Automaton::starts_with(prefix));
        
        while let Some((term, _)) = stream.next() {
            if let Ok(term_str) = std::str::from_utf8(term) {
                if let Some(suggestions) = self.suggestions.get(term_str) {
                    results.extend(suggestions.iter().cloned());
                    if results.len() >= limit { break; }
                }
            }
        }
        
        results.sort_by_key(|s| std::cmp::Reverse(s.score));
        results.truncate(limit);
        results
    }
}
```

**3. Búsqueda Difusa con Tolerancia a Errores**
```rust
// Búsqueda Levenshtein/Damerau-Levenshtein
struct FuzzySearchConfig {
    max_edits: u8,
    prefix_length: usize,
    transpositions: bool, // Damerau-Levenshtein si es true
}

fn build_fuzzy_query(field: &str, term: &str, config: &FuzzySearchConfig) -> Box<dyn Query> {
    let tantivy_term = Term::from_field_text(field, term);
    
    if config.transpositions {
        FuzzyTermQuery::new_damerau_levenshtein(
            tantivy_term, config.max_edits, config.prefix_length, true
        )
    } else {
        FuzzyTermQuery::new(tantivy_term, config.max_edits, config.prefix_length, true)
    }
}
```

**4. BM25 Mejorado con Scoring Personalizado**
```rust
// Factores de scoring personalizados
enum CustomScoreFactor {
    DownloadBoost { base: f32, scale: f32 },
    RecencyBoost { half_life: f32 },
    PopularityBoost { max_downloads: u64 },
    MaintainerReputation { multiplier: f32 },
}

impl CustomScoreFactor {
    fn compute(&self, doc: DocId) -> f32 {
        match self {
            Self::DownloadBoost { base, scale } => {
                let downloads = get_download_count(doc);
                base + (downloads as f32 / scale).ln_1p()
            }
            Self::RecencyBoost { half_life } => {
                let age_days = get_age_in_days(doc);
                (-age_days / half_life).exp()
            }
            Self::PopularityBoost { max_downloads } => {
                let downloads = get_download_count(doc);
                (downloads as f32 / *max_downloads as f32).sqrt()
            }
            Self::MaintainerReputation { multiplier } => {
                let maintainer_score = get_maintainer_score(doc);
                1.0 + (maintainer_score * multiplier)
            }
        }
    }
}
```

**5. Implementación de Búsqueda Facetada**
```rust
// Búsqueda facetada dinámica con Tantivy
async fn faceted_search(query: &AdvancedSearchQuery, searcher: &Searcher) -> FacetedSearchResult {
    let mut query_builder = BooleanQuery::builder();
    
    // Búsqueda de texto
    if !query.q.is_empty() {
        let text_query = build_text_query(&query.q);
        query_builder.add_occurrence(Occur::Must, text_query);
    }
    
    // Filtros de facetas
    for (facet_field, values) in &query.facets {
        let facet_query = build_facet_query(facet_field, values);
        query_builder.add_occurrence(Occur::Must, facet_query);
    }
    
    let final_query = query_builder.build();
    
    // Ejecutar con colección de facetas
    let facet_collector = FacetCollector::for_field("package_type")
        .with_field("repository")
        .with_field("license")
        .with_field("tags");
    
    let (top_docs, facet_counts) = searcher.search(&final_query, &facet_collector)?;
    
    FacetedSearchResult {
        results: convert_documents(top_docs),
        facets: extract_facets(facet_counts),
        total: top_docs.len(),
    }
}
```

### Estrategias de Optimización de Performance

**1. Caching Distribuido con Redis**
```rust
struct SearchCache {
    redis: RedisClient,
    local_cache: DashMap<String, CachedSearchResult>,
}

impl SearchCache {
    async fn get_cached_results(&self, cache_key: &str) -> Option<CachedSearchResult> {
        // Cache local primero
        if let Some(cached) = self.local_cache.get(cache_key) {
            return Some(cached.value().clone());
        }
        
        // Fallback a Redis
        if let Ok(Some(cached)) = self.redis.get(cache_key).await {
            self.local_cache.insert(cache_key.to_string(), cached.clone());
            return Some(cached);
        }
        
        None
    }
}
```

**2. Pipeline de Optimización de Queries**
```rust
struct QueryOptimizer {
    stop_words: HashSet<String>,
    synonym_expander: SynonymExpander,
    query_rewrite_rules: Vec<QueryRewriteRule>,
}

impl QueryOptimizer {
    fn optimize_query(&self, original_query: &str) -> (String, Vec<QueryTransformation>) {
        let mut query = original_query.to_lowercase();
        let mut transformations = Vec::new();
        
        // Remover stop words
        let (cleaned_query, stop_words_removed) = self.remove_stop_words(&query);
        if stop_words_removed > 0 {
            query = cleaned_query;
            transformations.push(QueryTransformation::StopWordsRemoved(stop_words_removed));
        }
        
        // Expansión de sinónimos
        let (expanded_query, synonyms_added) = self.synonym_expander.expand(&query);
        if synonyms_added > 0 {
            query = expanded_query;
            transformations.push(QueryTransformation::SynonymsExpanded(synonyms_added));
        }
        
        (query, transformations)
    }
}
```

### Patrones de Integración

**1. Wrapper Async-compatible para Tantivy**
```rust
struct AsyncTantivyIndex {
    index: Index,
    reader: IndexReader,
    writer: Arc<RwLock<IndexWriter>>,
    cpu_pool: Arc<Executor>, // Para operaciones CPU-intensivas
}

impl AsyncTantivyIndex {
    async fn search_async(&self, query: impl Query + Send + 'static) -> SearchResult<Vec<ArtifactSearchDocument>> {
        let searcher = self.reader.searcher();
        
        self.cpu_pool.spawn_blocking(move || {
            let top_docs = searcher.search(&query, &TopDocs::with_limit(100))?;
            
            let mut results = Vec::new();
            for (score, doc_address) in top_docs {
                let tantivy_doc = searcher.doc(doc_address)?;
                let artifact_doc = convert_tantivy_document(&tantivy_doc, score)?;
                results.push(artifact_doc);
            }
            
            Ok(results)
        }).await.map_err(|e| SearchError::execution_failed(e.to_string()))?
    }
}
```

**2. Analytics de Búsqueda Event-driven**
```rust
struct QueryAnalyticsPipeline {
    event_producer: KafkaProducer,
    redis: RedisClient,
    patterns: QueryPatternDetector,
}

impl QueryAnalyticsPipeline {
    async fn track_query(&self, query: &str, results: &SearchResult, user_id: Option<UserId>) {
        let event = QueryEvent {
            query: query.to_string(),
            result_count: results.total_count,
            search_time_ms: results.search_time_ms,
            user_id,
            timestamp: Utc::now(),
            filters: extract_filters_from_query(query),
        };
        
        // Analytics en tiempo real en Redis
        self.redis.increment_query_count(query).await;
        
        // Procesamiento batch via Kafka
        self.event_producer.send("query-events", &event).await;
        
        // Detección de patrones
        self.patterns.analyze_query_pattern(&event).await;
    }
}
```

### Benchmarks de Performance Esperados
- **Throughput de indexación**: 10K-50K docs/seg (single node)
- **Latencia de queries**: <10ms p95 para queries típicas
- **Uso de memoria**: ~100-500MB per 1M documentos
- **Cache hit rate**: >80% para queries frecuentes
- **Latencia de autocompletado**: <2ms para prefix matching

### Enfoque de Escalabilidad
1. **Fase 1**: Optimización single-node (Q2 2025)
2. **Fase 2**: Sharding de índices por repositorio (Q3 2025)
3. **Fase 3**: Federación de búsqueda distribuida (Q4 2025)
4. **Fase 4**: Recomendaciones powered por ML (2026)

### Prioridad de Implementación
Basado en la lista de features de Epic E3:

1. **P0**: Búsqueda facetada mejorada + indexación tiempo real (E3.F04, E3.F09)
2. **P1**: Autocompletado/sugerencias + búsqueda difusa (E3.F05, E3.F22)
3. **P1**: Scoring de relevancia + ranking (E3.F10, E3.F15)
4. **P2**: Analytics de queries + monitoreo performance (E3.F06, E3.F20)
5. **P2**: Búsqueda distribuida + caching (E3.F17, E3.F21)
6. **P3**: Recomendaciones ML + personalización (E3.F22)

Este enfoque asegura que la funcionalidad core de búsqueda se implemente primero mientras se construye hacia el set completo de 22 features descrito en Epic E3.

---
