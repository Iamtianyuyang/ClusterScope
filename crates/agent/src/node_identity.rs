use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct NodeIdentity {
    pub id: String,
}

impl NodeIdentity {
    pub fn load_or_create(path: &Path) -> Result<Self> {
        if path.exists() {
            let id = fs::read_to_string(path)
                .with_context(|| format!("Failed to read node identity from {:?}", path))?
                .trim()
                .to_string();
            
            if !id.is_empty() {
                return Ok(Self { id });
            }
        }
        
        // Generate new identity
        let new_id = format!("gpu-node-{}", Uuid::new_v4().simple());
        fs::write(path, &new_id)
            .with_context(|| format!("Failed to write node identity to {:?}", path))?;
        
        Ok(Self { id: new_id })
    }
}
