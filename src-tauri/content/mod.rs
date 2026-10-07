use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Content {
    #[serde(default = "new_article_id")]
    pub article_id: String,
    pub format: String,
    pub source_path: Option<String>,
    pub title: String,
    pub slug: String,
    pub summary: String,
    pub body: String,
    pub cover: Option<String>,
    pub authors: Vec<String>,
    pub tags: Vec<String>,
    pub metadata: Map<String, Value>,
}
impl Default for Content {
    fn default() -> Self {
        Self {
            article_id: new_article_id(),
            format: "markdown".into(),
            source_path: None,
            title: String::new(),
            slug: String::new(),
            summary: String::new(),
            body: String::new(),
            cover: None,
            authors: vec![],
            tags: vec![],
            metadata: Map::new(),
        }
    }
}
pub fn new_article_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
impl Content {
    pub fn ensure_id(&mut self) {
        if self.article_id.is_empty() {
            self.article_id = new_article_id();
        }
    }
    pub fn validate(&self) -> crate::core::Result<()> {
        if self.title.trim().is_empty() || self.body.trim().is_empty() {
            return Err(crate::core::Error::new("validation", "标题和正文不能为空"));
        }
        if !matches!(self.format.as_str(), "" | "markdown" | "mdx") {
            return Err(crate::core::Error::new(
                "validation",
                "format 必须为 markdown 或 mdx",
            ));
        }
        if self.body.len() > 2_000_000 {
            return Err(crate::core::Error::new("validation", "正文超过 2 MB"));
        }
        if !self.article_id.is_empty() && uuid::Uuid::parse_str(&self.article_id).is_err() {
            return Err(crate::core::Error::new(
                "validation",
                "article_id 必须为 UUID",
            ));
        }
        Ok(())
    }
}
