use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct FrequencyStore {
    counts: HashMap<String, u64>,
}

impl FrequencyStore {
    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }

    pub fn increment(&mut self, branch: &str) {
        *self.counts.entry(branch.to_string()).or_insert(0) += 1;
    }

    pub fn count(&self, branch: &str) -> u64 {
        self.counts.get(branch).copied().unwrap_or(0)
    }
}
