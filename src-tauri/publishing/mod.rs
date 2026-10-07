use crate::{
    content::Content,
    core::{Config, Error, Result},
    transform::Prepared,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path, sync::Arc};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    CreateDraft,
    Publish,
    Update,
    Delete,
}
impl Action {
    pub fn capability(self) -> &'static str {
        match self {
            Self::CreateDraft => "draft",
            Self::Publish => "publish",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub capabilities: Vec<String>,
    pub schema: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Receipt {
    pub id: String,
    pub url: Option<String>,
    pub status: String,
    #[serde(default)]
    pub revision: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteAsset {
    pub id: String,
    pub url: String,
}
pub mod reliable;
#[async_trait]
pub trait Publisher: Send + Sync {
    fn resolve_asset_source(
        &self,
        source: &str,
        _p: &Prepared,
        _action: Action,
        _cfg: &Config,
    ) -> Result<String> {
        Ok(source.into())
    }
    fn target_identity(&self, cfg: &Config) -> Value {
        let mut cfg = cfg.clone();
        for (key, schema) in self.manifest().schema["properties"]
            .as_object()
            .into_iter()
            .flatten()
        {
            if schema["secret"] == true {
                cfg.remove(key);
            }
        }
        serde_json::json!(cfg)
    }
    fn asset_variant(&self, cover: bool, _p: &Prepared, _action: Action, _cfg: &Config) -> String {
        if cover {
            "cover".into()
        } else {
            "body".into()
        }
    }
    async fn upload_asset(
        &self,
        _asset: crate::transform::Asset,
        _variant: &str,
        _cfg: &Config,
    ) -> Result<RemoteAsset> {
        Err(Error::unsupported())
    }
    fn safe_remote_retry(&self, _cfg: &Config) -> bool {
        false
    }
    async fn recover(
        &self,
        _p: &Prepared,
        _action: Action,
        _cfg: &Config,
    ) -> Result<Option<Receipt>> {
        Ok(None)
    }
    fn manifest(&self) -> Manifest;
    fn capabilities(&self) -> Vec<String> {
        self.manifest().capabilities
    }
    async fn connect(&self, cfg: &Config) -> Result<()> {
        self.test_connection(cfg).await
    }
    async fn test_connection(&self, cfg: &Config) -> Result<()>;
    async fn transform(&self, content: &Content, _cfg: &Config) -> Result<Prepared> {
        Ok(crate::transform::markdown(content))
    }
    async fn preview(&self, content: &Content, cfg: &Config) -> Result<Prepared> {
        self.transform(content, cfg).await
    }
    async fn execute(
        &self,
        action: Action,
        prepared: Prepared,
        cfg: &Config,
        base: Option<&Path>,
        remote_id: Option<&str>,
    ) -> Result<Receipt>;
    async fn create_draft(
        &self,
        p: Prepared,
        cfg: &Config,
        base: Option<&Path>,
    ) -> Result<Receipt> {
        self.execute(Action::CreateDraft, p, cfg, base, None).await
    }
    async fn publish(&self, p: Prepared, cfg: &Config, base: Option<&Path>) -> Result<Receipt> {
        self.execute(Action::Publish, p, cfg, base, None).await
    }
    async fn update(
        &self,
        p: Prepared,
        cfg: &Config,
        base: Option<&Path>,
        id: &str,
    ) -> Result<Receipt> {
        self.execute(Action::Update, p, cfg, base, Some(id)).await
    }
}
pub struct Registry {
    publishers: BTreeMap<String, Arc<dyn Publisher>>,
}
impl Registry {
    pub fn builtin() -> Self {
        Self::new(crate::extensions::builtins())
    }
    pub fn new(publishers: Vec<Arc<dyn Publisher>>) -> Self {
        let mut registry = Self {
            publishers: BTreeMap::new(),
        };
        for p in publishers {
            registry.publishers.insert(p.manifest().id.clone(), p);
        }
        registry
    }
    pub fn manifests(&self) -> Vec<Manifest> {
        self.publishers.values().map(|p| p.manifest()).collect()
    }
    pub fn get(&self, id: &str) -> Result<Arc<dyn Publisher>> {
        self.publishers
            .get(id)
            .cloned()
            .ok_or_else(|| Error::new("extension", "未知发布扩展"))
    }
    pub async fn dispatch(
        &self,
        id: &str,
        action: Action,
        content: &Content,
        cfg: &Config,
        base: Option<&Path>,
        remote_id: Option<&str>,
    ) -> Result<Receipt> {
        let publisher = self.get(id)?;
        if !publisher
            .capabilities()
            .iter()
            .any(|c| c == action.capability())
        {
            return Err(Error::unsupported());
        }
        content.validate()?;
        let prepared = publisher.transform(content, cfg).await?;
        match action {
            Action::CreateDraft => publisher.create_draft(prepared, cfg, base).await,
            Action::Publish => publisher.publish(prepared, cfg, base).await,
            Action::Update => {
                publisher
                    .update(
                        prepared,
                        cfg,
                        base,
                        remote_id
                            .filter(|s| !s.is_empty())
                            .ok_or_else(|| Error::new("validation", "更新需要远端文章 ID"))?,
                    )
                    .await
            }
            Action::Delete => {
                publisher
                    .execute(action, prepared, cfg, base, remote_id)
                    .await
            }
        }
    }
}
