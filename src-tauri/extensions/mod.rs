pub mod generic_rest;
pub mod git_content;
pub mod wechat;
pub mod wordpress;
use crate::publishing::{Manifest, Publisher};
use serde_json::{json, Value};
use std::sync::Arc;
pub fn builtins() -> Vec<Arc<dyn Publisher>> {
    vec![
        Arc::new(wordpress::WordPress::new()),
        Arc::new(wechat::WeChat::new()),
        Arc::new(generic_rest::GenericRest::new()),
        Arc::new(git_content::GitContent),
    ]
}
pub fn manifest(
    id: &str,
    name: &str,
    description: &str,
    caps: &[&str],
    properties: Value,
    required: &[&str],
) -> Manifest {
    Manifest {
        id: id.into(),
        name: name.into(),
        version: "0.2.0".into(),
        description: description.into(),
        capabilities: caps.iter().map(|s| s.to_string()).collect(),
        schema: json!({"type":"object", "properties":properties,"required":required}),
    }
}
