// kernel/src/model_fabric.rs
//! Model Fabric: unified abstraction over all AI model types.
//!
//! The Model Fabric allows applications to declare model requirements by
//! capability (text generation, vision, speech, etc.) rather than by provider.
//! The fabric selects the best available model and automatically fails over
//! to alternatives when the primary is unavailable.
//!
//! ## Failover chain (AILang syntax)
//!
//! ```ail
//! model primary GPT5
//! model fallback Claude
//! model emergency LocalModel
//! ```
//!
//! If GPT5 is unavailable → Claude; if Claude is unavailable → LocalModel.
//! The mock model is always the ultimate fallback.
//!
//! ## V3.0 honest limitations
//!
//! Model selection happens at the metadata level: the fabric chooses a model
//! name. Actual inference is still handled by `ailang-ai-runtime`'s
//! `ModelProvider` implementations (`MockProvider`, `HttpModelProvider`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

/// A capability that a model can possess.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ModelCapability {
    TextGeneration,
    ImageUnderstanding,
    SpeechRecognition,
    SpeechSynthesis,
    Embedding,
    CodeGeneration,
    Reasoning,
    MultiModal,
    Classification,
    ObjectDetection,
}

/// Where the model is deployed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ModelLocation {
    Local { path: String },
    Cloud { endpoint: String },
    Edge { device_id: String },
    /// The built-in AILang mock model (always available, never calls a real API).
    Mock,
}

/// Availability status of a model.
#[derive(Debug, Clone, PartialEq)]
pub enum ModelStatus {
    Available,
    Unavailable { reason: String },
    Overloaded,
    RateLimited { retry_after_ms: u64 },
}

/// Full specification for a model registered with the fabric.
#[derive(Debug, Clone)]
pub struct ModelSpec {
    pub name: String,
    pub capabilities: Vec<ModelCapability>,
    pub location: ModelLocation,
    pub status: ModelStatus,
    /// Estimated cost per token (USD). `None` for local/mock models.
    pub cost_per_token: Option<f64>,
    /// Observed p50 latency in milliseconds.
    pub latency_p50_ms: Option<u64>,
    /// Maximum context window size (tokens).
    pub context_window: Option<usize>,
    /// Fallback models in priority order (names must be registered).
    pub fallbacks: Vec<String>,
}

/// How to handle primary model failure.
#[derive(Debug, Clone)]
pub enum FailoverPolicy {
    /// Try fallbacks sequentially until one succeeds.
    Sequential,
    /// Use the mock model as the unconditional last resort.
    AlwaysMock,
}

/// The unified model registry with automatic failover.
pub struct ModelFabric {
    models: Arc<Mutex<HashMap<String, ModelSpec>>>,
    primary: Arc<Mutex<Option<String>>>,
    fallback: Arc<Mutex<Option<String>>>,
    emergency: Arc<Mutex<Option<String>>>,
    pub failover_policy: FailoverPolicy,
}

impl ModelFabric {
    pub fn new() -> Self {
        let mut models = HashMap::new();
        // The mock model is always registered and always available.
        models.insert(
            "mock".to_string(),
            ModelSpec {
                name: "mock".to_string(),
                capabilities: vec![
                    ModelCapability::TextGeneration,
                    ModelCapability::Embedding,
                    ModelCapability::Classification,
                    ModelCapability::CodeGeneration,
                ],
                location: ModelLocation::Mock,
                status: ModelStatus::Available,
                cost_per_token: Some(0.0),
                latency_p50_ms: Some(1),
                context_window: Some(1_000_000),
                fallbacks: vec![],
            },
        );

        Self {
            models: Arc::new(Mutex::new(models)),
            primary: Arc::new(Mutex::new(None)),
            fallback: Arc::new(Mutex::new(None)),
            emergency: Arc::new(Mutex::new(None)),
            failover_policy: FailoverPolicy::Sequential,
        }
    }

    /// Register a model with the fabric.
    pub fn register(&self, spec: ModelSpec) {
        self.models.lock().unwrap().insert(spec.name.clone(), spec);
    }

    pub fn set_primary(&self, name: &str) {
        *self.primary.lock().unwrap() = Some(name.to_string());
    }

    pub fn set_fallback(&self, name: &str) {
        *self.fallback.lock().unwrap() = Some(name.to_string());
    }

    pub fn set_emergency(&self, name: &str) {
        *self.emergency.lock().unwrap() = Some(name.to_string());
    }

    /// Select the best available model for a given capability.
    ///
    /// Tries: primary → declared fallback → emergency → any available → mock.
    pub fn select(&self, capability: &ModelCapability) -> Option<String> {
        let models = self.models.lock().unwrap();

        let candidates: Vec<Option<String>> = vec![
            self.primary.lock().unwrap().clone(),
            self.fallback.lock().unwrap().clone(),
            self.emergency.lock().unwrap().clone(),
            Some("mock".to_string()),
        ];

        for candidate in candidates.into_iter().flatten() {
            if let Some(spec) = models.get(&candidate) {
                if spec.status == ModelStatus::Available
                    && spec.capabilities.contains(capability)
                {
                    return Some(candidate);
                }
            }
        }

        // Last resort: scan all models for the capability
        models
            .values()
            .find(|m| {
                m.status == ModelStatus::Available && m.capabilities.contains(capability)
            })
            .map(|m| m.name.clone())
    }

    /// Mark a model as unavailable (e.g. on API error).
    pub fn mark_unavailable(&self, name: &str, reason: &str) {
        let mut models = self.models.lock().unwrap();
        if let Some(spec) = models.get_mut(name) {
            spec.status = ModelStatus::Unavailable {
                reason: reason.to_string(),
            };
        }
    }

    /// Mark a model as available again (e.g. after a health check passes).
    pub fn mark_available(&self, name: &str) {
        let mut models = self.models.lock().unwrap();
        if let Some(spec) = models.get_mut(name) {
            spec.status = ModelStatus::Available;
        }
    }

    pub fn list(&self) -> Vec<ModelSpec> {
        self.models.lock().unwrap().values().cloned().collect()
    }

    pub fn get(&self, name: &str) -> Option<ModelSpec> {
        self.models.lock().unwrap().get(name).cloned()
    }

    /// Check if a specific model is currently available.
    pub fn is_available(&self, name: &str) -> bool {
        self.models
            .lock()
            .unwrap()
            .get(name)
            .map(|m| m.status == ModelStatus::Available)
            .unwrap_or(false)
    }
}

impl Default for ModelFabric {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_gen_spec(name: &str) -> ModelSpec {
        ModelSpec {
            name: name.to_string(),
            capabilities: vec![ModelCapability::TextGeneration],
            location: ModelLocation::Cloud {
                endpoint: format!("http://localhost/{}", name),
            },
            status: ModelStatus::Available,
            cost_per_token: Some(0.001),
            latency_p50_ms: Some(200),
            context_window: Some(4096),
            fallbacks: vec![],
        }
    }

    #[test]
    fn mock_always_available() {
        let fabric = ModelFabric::new();
        let selected = fabric.select(&ModelCapability::TextGeneration);
        assert_eq!(selected, Some("mock".to_string()));
    }

    #[test]
    fn primary_selected_when_available() {
        let fabric = ModelFabric::new();
        fabric.register(text_gen_spec("GPT5"));
        fabric.set_primary("GPT5");
        assert_eq!(
            fabric.select(&ModelCapability::TextGeneration),
            Some("GPT5".to_string())
        );
    }

    #[test]
    fn failover_to_fallback_when_primary_unavailable() {
        let fabric = ModelFabric::new();
        fabric.register(text_gen_spec("GPT5"));
        fabric.register(text_gen_spec("Claude"));
        fabric.set_primary("GPT5");
        fabric.set_fallback("Claude");

        fabric.mark_unavailable("GPT5", "API error");

        assert_eq!(
            fabric.select(&ModelCapability::TextGeneration),
            Some("Claude".to_string())
        );
    }

    #[test]
    fn failover_to_emergency_when_primary_and_fallback_unavailable() {
        let fabric = ModelFabric::new();
        fabric.register(text_gen_spec("GPT5"));
        fabric.register(text_gen_spec("Claude"));
        fabric.register(text_gen_spec("LocalModel"));
        fabric.set_primary("GPT5");
        fabric.set_fallback("Claude");
        fabric.set_emergency("LocalModel");

        fabric.mark_unavailable("GPT5", "error");
        fabric.mark_unavailable("Claude", "error");

        assert_eq!(
            fabric.select(&ModelCapability::TextGeneration),
            Some("LocalModel".to_string())
        );
    }

    #[test]
    fn mock_is_ultimate_fallback() {
        let fabric = ModelFabric::new();
        fabric.register(text_gen_spec("GPT5"));
        fabric.set_primary("GPT5");
        fabric.mark_unavailable("GPT5", "error");

        // No fallback/emergency set → should fall through to mock
        assert_eq!(
            fabric.select(&ModelCapability::TextGeneration),
            Some("mock".to_string())
        );
    }

    #[test]
    fn capability_mismatch_skips_model() {
        let fabric = ModelFabric::new();
        let vision_only = ModelSpec {
            name: "VisionModel".to_string(),
            capabilities: vec![ModelCapability::ImageUnderstanding],
            location: ModelLocation::Cloud { endpoint: "...".into() },
            status: ModelStatus::Available,
            cost_per_token: None,
            latency_p50_ms: None,
            context_window: None,
            fallbacks: vec![],
        };
        fabric.register(vision_only);
        fabric.set_primary("VisionModel");

        // VisionModel doesn't do TextGeneration → fall through to mock
        let selected = fabric.select(&ModelCapability::TextGeneration);
        assert_eq!(selected, Some("mock".to_string()));

        // But it does handle ImageUnderstanding
        let vis_selected = fabric.select(&ModelCapability::ImageUnderstanding);
        assert_eq!(vis_selected, Some("VisionModel".to_string()));
    }

    #[test]
    fn mark_available_restores_failover() {
        let fabric = ModelFabric::new();
        fabric.register(text_gen_spec("GPT5"));
        fabric.set_primary("GPT5");
        fabric.mark_unavailable("GPT5", "down");
        assert_eq!(fabric.select(&ModelCapability::TextGeneration), Some("mock".to_string()));

        fabric.mark_available("GPT5");
        assert_eq!(fabric.select(&ModelCapability::TextGeneration), Some("GPT5".to_string()));
    }
}

