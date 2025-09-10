use async_trait::async_trait;
use std::collections::HashMap;
use tracing::{debug, info, error};

use crate::features::full_text_search::{
    dto::{SearchQuery, SearchResults, ArtifactDocument},
    error::FullTextSearchError,
    ports::ScorerPort,
};

/// BM25 scorer for relevance ranking
pub struct BM25Scorer {
    k1: f32,
    b: f32,
    avg_doc_length: f32,
    total_docs: usize,
    doc_freqs: HashMap<String, usize>,
}

impl BM25Scorer {
    pub fn new(k1: f32, b: f32) -> Self {
        Self {
            k1,
            b,
            avg_doc_length: 100.0, // Placeholder value
            total_docs: 1000,      // Placeholder value
            doc_freqs: HashMap::new(),
        }
    }
    
    pub fn with_defaults() -> Self {
        Self::new(1.2, 0.75)
    }
    
    fn idf(&self, term_freq: usize) -> f32 {
        ((self.total_docs as f32 - term_freq as f32 + 0.5) / (term_freq as f32 + 0.5) + 1.0).ln()
    }
    
    fn bm25_score(&self, term_freq: f32, doc_length: usize) -> f32 {
        let numerator = term_freq * (self.k1 + 1.0);
        let denominator = term_freq + self.k1 * (1.0 - self.b + self.b * (doc_length as f32 / self.avg_doc_length));
        numerator / denominator
    }
}

#[async_trait]
impl ScorerPort for BM25Scorer {
    async fn calculate_score(
        &self,
        query_terms: &[String],
        document_terms: &[String],
        document_length: usize,
    ) -> Result<f32, FullTextSearchError> {
        debug!(
            query_term_count = query_terms.len(),
            document_term_count = document_terms.len(),
            document_length = document_length,
            "Calculating BM25 relevance score"
        );
        
        let mut score = 0.0f32;
        
        for query_term in query_terms {
            let term_freq = document_terms.iter().filter(|&t| t == query_term).count() as f32;
            if term_freq > 0.0 {
                let idf = self.idf(term_freq as usize);
                let bm25_component = self.bm25_score(term_freq, document_length);
                score += idf * bm25_component;
            }
        }
        
        Ok(score)
    }
    
    async fn normalize_scores(&self, scores: &[f32]) -> Result<Vec<f32>, FullTextSearchError> {
        debug!(score_count = scores.len(), "Normalizing scores");
        
        if scores.is_empty() {
            return Ok(vec![]);
        }
        
        let max_score = scores.iter().fold(0.0f32, |acc, &x| if x > acc { x } else { acc });
        
        if max_score == 0.0 {
            return Ok(scores.to_vec());
        }
        
        let normalized_scores: Vec<f32> = scores.iter().map(|&score| score / max_score).collect();
        Ok(normalized_scores)
    }
    
    async fn rank_results(
        &self,
        results: &mut SearchResults,
    ) -> Result<(), FullTextSearchError> {
        debug!(result_count = results.artifacts.len(), "Ranking search results with BM25");
        
        // In a real implementation, we would calculate scores for each result
        // and sort them by relevance
        // For now, we'll just set a default score
        for artifact in &mut results.artifacts {
            artifact.score = 1.0; // Placeholder score
        }
        
        // Sort by score descending
        results.artifacts.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        
        info!(result_count = results.artifacts.len(), "Search results ranked successfully");
        Ok(())
    }
}