use crate::{
    core::{self, Config, Error, Result},
    extensions::manifest,
    publishing::{Action, Manifest, Publisher, Receipt, RemoteAsset},
    transform::{self, Prepared},
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;

pub struct WordPress {
    client: reqwest::Client,
}
impl Default for WordPress {
    fn default() -> Self {
        Self::new()
    }
}
impl WordPress {
    pub fn new() -> Self {
        Self {
            client: core::client(),
        }
    }
    fn url(cfg: &Config, path: &str) -> Result<String> {
        Ok(format!(
            "{}/wp-json/wp/v2/{path}",
            core::endpoint(core::required(cfg, "url")?)?
        ))
    }
    fn auth(
        &self,
        request: reqwest::RequestBuilder,
        cfg: &Config,
    ) -> Result<reqwest::RequestBuilder> {
        Ok(request.basic_auth(
            core::required(cfg, "username")?,
            Some(core::required(cfg, "password")?),
        ))
    }
    async fn send(&self, request: reqwest::RequestBuilder, cfg: &Config) -> Result<Value> {
        core::json_response(
            self.auth(request, cfg)?
                .send()
                .await
                .map_err(core::network)?,
        )
        .await
    }
    async fn upload(&self, source: &str, cfg: &Config, base: Option<&Path>) -> Result<Value> {
        let asset = transform::load_asset(&self.client, source, base).await?;
        let part = reqwest::multipart::Part::bytes(asset.bytes)
            .file_name(asset.name)
            .mime_str(&asset.mime)
            .map_err(|_| Error::new("asset", "图片 MIME 类型无效"))?;
        self.send(
            self.client
                .post(Self::url(cfg, "media")?)
                .multipart(reqwest::multipart::Form::new().part("file", part)),
            cfg,
        )
        .await
    }
    async fn tag(&self, name: &str, cfg: &Config) -> Result<u64> {
        let found = self
            .send(
                self.client
                    .get(Self::url(cfg, "tags")?)
                    .query(&[("search", name), ("per_page", "100")]),
                cfg,
            )
            .await?;
        if let Some(id) = found
            .as_array()
            .and_then(|items| {
                items.iter().find(|v| {
                    v["name"]
                        .as_str()
                        .is_some_and(|s| s.eq_ignore_ascii_case(name))
                })
            })
            .and_then(|v| v["id"].as_u64())
        {
            return Ok(id);
        }
        let created = self
            .send(
                self.client
                    .post(Self::url(cfg, "tags")?)
                    .json(&json!({"name":name})),
                cfg,
            )
            .await?;
        created["id"]
            .as_u64()
            .ok_or_else(|| Error::new("api", "WordPress 标签响应缺少 ID"))
    }
}
#[async_trait]
impl Publisher for WordPress {
    fn target_identity(&self, cfg: &Config) -> Value {
        json!({"url":core::field(cfg,"url").trim_end_matches('/'),"username":core::field(cfg,"username")})
    }
    fn asset_variant(&self, _cover: bool, _p: &Prepared, _action: Action, _cfg: &Config) -> String {
        "image".into()
    }
    async fn upload_asset(
        &self,
        asset: transform::Asset,
        _variant: &str,
        cfg: &Config,
    ) -> Result<RemoteAsset> {
        let part = reqwest::multipart::Part::bytes(asset.bytes)
            .file_name(asset.name)
            .mime_str(&asset.mime)
            .map_err(|_| Error::new("asset", "图片 MIME 类型无效"))?;
        let response = self
            .send(
                self.client
                    .post(Self::url(cfg, "media")?)
                    .multipart(reqwest::multipart::Form::new().part("file", part)),
                cfg,
            )
            .await?;
        Ok(RemoteAsset {
            id: response["id"]
                .as_u64()
                .ok_or_else(|| Error::new("api", "图片响应缺少 ID"))?
                .to_string(),
            url: response["source_url"]
                .as_str()
                .ok_or_else(|| Error::new("api", "图片响应缺少 URL"))?
                .into(),
        })
    }
    async fn recover(
        &self,
        p: &Prepared,
        _action: Action,
        cfg: &Config,
    ) -> Result<Option<Receipt>> {
        let slug = if p.content.slug.is_empty() {
            format!("wb-{}", p.content.article_id)
        } else {
            p.content.slug.clone()
        };
        let result = self
            .send(
                self.client.get(Self::url(cfg, "posts")?).query(&[
                    ("slug", slug.as_str()),
                    ("context", "edit"),
                    ("status", "draft,publish,pending,private,future"),
                ]),
                cfg,
            )
            .await?;
        for item in result.as_array().into_iter().flatten() {
            if item["content"]["raw"].as_str().is_some_and(|s| {
                s.contains(&format!("workbench:{}:{}", p.content.article_id, p.job_id))
            }) {
                return Ok(Some(Receipt {
                    id: item["id"].to_string(),
                    url: item["link"].as_str().map(str::to_owned),
                    status: item["status"].as_str().unwrap_or("draft").into(),
                    revision: item["modified_gmt"].as_str().map(str::to_owned),
                }));
            }
        }
        Ok(None)
    }
    fn manifest(&self) -> Manifest {
        manifest(
            "wordpress",
            "WordPress",
            "官方 REST API · Application Password",
            &["draft", "publish", "update", "assets", "html", "tags"],
            json!({
                "url":{"type":"string","title":"站点 URL","placeholder":"https://example.com"},
                "username":{"type":"string","title":"用户名"},
                "password":{"type":"string","title":"Application Password","secret":true},
                "categories":{"type":"string","title":"默认 Category IDs（逗号分隔）","placeholder":"1,2"}
            }),
            &["url", "username", "password"],
        )
    }
    async fn test_connection(&self, cfg: &Config) -> Result<()> {
        self.send(
            self.client
                .get(Self::url(cfg, "users/me")?)
                .query(&[("context", "edit")]),
            cfg,
        )
        .await?;
        Ok(())
    }
    async fn execute(
        &self,
        action: Action,
        mut p: Prepared,
        cfg: &Config,
        base: Option<&Path>,
        remote_id: Option<&str>,
    ) -> Result<Receipt> {
        if action == Action::Delete {
            return Err(Error::unsupported());
        }
        if !p.assets_processed {
            for source in transform::image_sources(&p.content.body) {
                let asset = self.upload(&source, cfg, base).await?;
                let url = asset["source_url"]
                    .as_str()
                    .ok_or_else(|| Error::new("api", "WordPress 图片响应缺少 URL"))?;
                transform::replace_image(&mut p, &source, url);
            }
        }
        let cover = if let Some(asset) = &p.cover_asset {
            Some(
                asset
                    .id
                    .parse::<u64>()
                    .map_err(|_| Error::new("asset", "封面 ID 无效"))?,
            )
        } else if !p.assets_processed {
            if let Some(source) = p.content.cover.as_deref().filter(|s| !s.is_empty()) {
                Some(
                    self.upload(source, cfg, base).await?["id"]
                        .as_u64()
                        .ok_or_else(|| Error::new("api", "WordPress 封面响应缺少 ID"))?,
                )
            } else {
                None
            }
        } else {
            None
        };
        let mut tags = Vec::new();
        for name in &p.content.tags {
            tags.push(self.tag(name, cfg).await?);
        }
        let categories: Vec<u64> = if let Some(value) = p.content.metadata.get("categories") {
            serde_json::from_value(value.clone())
                .map_err(|_| Error::new("validation", "metadata.categories 必须为数字 ID 数组"))?
        } else {
            core::field(cfg, "categories")
                .split(',')
                .filter(|s| !s.trim().is_empty())
                .map(|s| {
                    s.trim()
                        .parse()
                        .map_err(|_| Error::new("configuration", "Category ID 必须为数字"))
                })
                .collect::<Result<_>>()?
        };
        if !p.job_id.is_empty() {
            p.html.push_str(&format!(
                "\n<!-- workbench:{}:{} -->",
                p.content.article_id, p.job_id
            ));
            if p.content.slug.is_empty() {
                p.content.slug = format!("wb-{}", p.content.article_id);
            }
        }
        let mut payload = json!({"title":p.content.title,"slug":p.content.slug,"excerpt":p.content.summary,"content":p.html,"tags":tags,"categories":categories});
        match action {
            Action::CreateDraft => payload["status"] = json!("draft"),
            Action::Publish => payload["status"] = json!("publish"),
            Action::Update => {
                match p
                    .content
                    .metadata
                    .get("_workbench_intent")
                    .and_then(Value::as_str)
                {
                    Some("createDraft") => payload["status"] = json!("draft"),
                    Some("publish") => payload["status"] = json!("publish"),
                    _ => {}
                }
            }
            Action::Delete => return Err(Error::unsupported()),
        }
        if let Some(id) = cover {
            payload["featured_media"] = json!(id);
        }
        if let Some(id) = p.content.metadata.get("author_id") {
            let id = id
                .as_u64()
                .ok_or_else(|| Error::new("validation", "metadata.author_id 必须为数字"))?;
            payload["author"] = json!(id);
        }
        let path = if action == Action::Update {
            let id = remote_id.ok_or_else(|| Error::new("validation", "更新需要文章 ID"))?;
            if id.parse::<u64>().is_err() {
                return Err(Error::new("validation", "WordPress ID 必须为数字"));
            }
            format!("posts/{id}")
        } else {
            "posts".into()
        };
        let response = self
            .send(self.client.post(Self::url(cfg, &path)?).json(&payload), cfg)
            .await?;
        Ok(Receipt {
            id: response["id"]
                .as_u64()
                .ok_or_else(|| Error::new("api", "WordPress 响应缺少文章 ID"))?
                .to_string(),
            url: response["link"].as_str().map(str::to_owned),
            status: response["status"].as_str().unwrap_or("updated").into(),
            revision: response["modified_gmt"].as_str().map(str::to_owned),
        })
    }
}
