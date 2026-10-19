//! Mesh app preferences the panel edits: which ladder model to share, its
//! price, and "share now". Stored as `~/.mesh-app/mesh-app.toml`; the tray
//! reads it, picks the model and applies the price once the model is served.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct Price {
    pub input_sats: u64,
    pub output_sats: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct AppConfig {
    /// Ladder model to share; `None` = automatic hardware pick.
    pub model: Option<String>,
    /// Ignore "Share when" rules and share now.
    pub share_now: bool,
    /// Price per model, sats per 1M tokens. Absent = ladder suggestion;
    /// 0/0 = free.
    pub prices: BTreeMap<String, Price>,
}

pub const FREE: Price = Price {
    input_sats: 0,
    output_sats: 0,
};

pub struct Rung {
    pub label: &'static str,
    pub model: &'static str,
    pub suggested: Price,
}

/// Same rungs as the automatic pick, largest first. Suggested prices are
/// sats per 1M tokens, from hosted prices at $82.3k/BTC (2026-10-19):
/// OpenRouter Qwen3.8-27B, SiliconFlow Gemma 4 12B, OpenRouter Qwen3.5-9B.
pub const LADDER: [Rung; 3] = [
    Rung {
        label: "Qwen3.8 27B",
        model: crate::model_selection::LARGE,
        suggested: Price {
            input_sats: 250,
            output_sats: 2000,
        },
    },
    Rung {
        label: "Gemma 4 12B",
        model: crate::model_selection::GEMMA,
        suggested: Price {
            input_sats: 125,
            output_sats: 375,
        },
    },
    Rung {
        label: "Qwen3.5 4B",
        model: crate::model_selection::SMALL,
        suggested: Price {
            input_sats: 100,
            output_sats: 150,
        },
    },
];

pub fn path(root: &Path) -> PathBuf {
    root.join("mesh-app.toml")
}

impl AppConfig {
    pub fn load(root: &Path) -> Self {
        std::fs::read_to_string(path(root))
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, root: &Path) -> Result<(), String> {
        let text = toml::to_string(self).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
        let tmp = path(root).with_extension("toml.tmp");
        std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path(root)).map_err(|e| e.to_string())
    }

    /// Price to serve `model` at: the saved one, else the ladder suggestion.
    /// A saved 0/0 means "free" (`None`).
    pub fn price_for(&self, model: &str) -> Option<Price> {
        let p = self.prices.get(model).copied().or_else(|| {
            LADDER
                .iter()
                .find(|r| r.model == model)
                .map(|r| r.suggested)
        })?;
        (p != FREE).then_some(p)
    }

    /// The chosen model, only if it is a ladder rung that fits this machine.
    pub fn chosen_model(&self) -> Option<&'static str> {
        let want = self.model.as_deref()?;
        LADDER
            .iter()
            .find(|r| r.model == want && crate::model_selection::fits(r.model))
            .map(|r| r.model)
    }
}

/// Short display name for a model ref.
pub fn label(model: &str) -> &str {
    LADDER
        .iter()
        .find(|r| r.model == model)
        .map(|r| r.label)
        .unwrap_or(model)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trips_and_ignores_unknown_models() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = AppConfig {
            model: Some("not-on-ladder".into()),
            share_now: true,
            ..Default::default()
        };
        c.prices.insert(
            "m".into(),
            Price {
                input_sats: 1,
                output_sats: 2,
            },
        );
        c.save(dir.path()).unwrap();
        let back = AppConfig::load(dir.path());
        assert_eq!(back, c);
        assert_eq!(back.chosen_model(), None);
        let rung = LADDER[0].model;
        assert_eq!(back.price_for(rung), Some(LADDER[0].suggested));
        let mut c = back;
        c.prices.insert(rung.into(), FREE);
        assert_eq!(c.price_for(rung), None);
    }
}
