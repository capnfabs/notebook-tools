//! The Claude models that transcribe pages, and what they cost.
//!
//! See `decisions/2026-10-24-model-selector.md` and `decisions/2026-10-24-sonnet-quality.md`.

use serde::{Deserialize, Serialize};

/// Which Claude transcribes. Both see images at the same resolution, so [`crate::ocr::MAX_IMAGE_PIXELS`]
/// suits
/// either.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Model {
    #[default]
    Opus,
    Sonnet,
}

impl Model {
    pub const ALL: [Self; 2] = [Self::Opus, Self::Sonnet];

    /// The API's name for it.
    pub fn id(self) -> &'static str {
        match self {
            Self::Opus => "claude-opus-5-5",
            Self::Sonnet => "claude-sonnet-5-5",
        }
    }

    /// Its family, without version or effort: names its [`crate::cache`] entries.
    pub fn name(self) -> &'static str {
        match self {
            Self::Opus => "opus",
            Self::Sonnet => "sonnet",
        }
    }

    /// The family of the model the API names `id`, which may be a fallback's.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|m| id.starts_with(&format!("claude-{}", m.name())))
    }

    /// Dollars per million input and output tokens.
    pub fn prices(self) -> (f64, f64) {
        match self {
            Self::Opus => (4.0, 20.0),
            Self::Sonnet => (2.0, 10.0),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(from = "RawUsage")]
pub struct Usage {
    pub input_tokens: u64,
    /// Including thinking.
    pub output_tokens: u64,
    pub thinking_tokens: u64,
}

impl Usage {
    /// What it cost in dollars, at `model`'s prices.
    pub fn cost(&self, model: Model) -> f64 {
        let (input, output) = model.prices();
        (self.input_tokens as f64 * input + self.output_tokens as f64 * output) / 1e6
    }
}

#[derive(Deserialize)]
struct RawUsage {
    input_tokens: u64,
    output_tokens: u64,
    #[serde(default)]
    output_tokens_details: Option<OutputTokensDetails>,
}

#[derive(Deserialize)]
struct OutputTokensDetails {
    #[serde(default)]
    thinking_tokens: u64,
}

impl From<RawUsage> for Usage {
    fn from(raw: RawUsage) -> Self {
        Self {
            input_tokens: raw.input_tokens,
            output_tokens: raw.output_tokens,
            thinking_tokens: raw.output_tokens_details.map_or(0, |d| d.thinking_tokens),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn models_by_id_and_cost() {
        assert_eq!(Model::from_id("claude-opus-5-5"), Some(Model::Opus));
        assert_eq!(
            Model::from_id("claude-sonnet-5"),
            Some(Model::Sonnet),
            "a fallback"
        );
        assert_eq!(Model::from_id("claude-haiku-5-5"), None);
        let usage = Usage {
            input_tokens: 5_000,
            output_tokens: 2_000,
            thinking_tokens: 800,
        };
        assert!((usage.cost(Model::Opus) - 0.06).abs() < 1e-9);
        assert!((usage.cost(Model::Sonnet) - 0.03).abs() < 1e-9);
    }
}
