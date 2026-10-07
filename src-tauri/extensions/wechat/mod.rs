use crate::{
    content::Content,
    core::{self, Config, Error, Result},
    extensions::manifest,
    publishing::{Action, Manifest, Publisher, Receipt, RemoteAsset},
    transform::{self, Prepared},
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::Path,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

pub struct WeChat {
    client: reqwest::Client,
    base: String,
    tokens: Mutex<HashMap<String, (String, String, Instant)>>,
}
impl Default for WeChat {
    fn default() -> Self {
        Self::new()
    }
}
impl WeChat {
    pub fn new() -> Self {
        Self::with_base("https://api.weixin.qq.com")
    }
    // Backend-only injection for API mocks; not exposed in configuration.
    pub fn with_base(base: &str) -> Self {
        Self {
            client: core::client(),
            base: base.into(),
            tokens: Mutex::new(HashMap::new()),
        }
    }
    async fn token(&self, cfg: &Config) -> Result<String> {
        let appid = core::required(cfg, "appid")?;
        let secret = core::required(cfg, "secret")?;
        // Serialize token refresh to avoid concurrent invalidation.
        let mut tokens = self.tokens.lock().await;
        if let Some((cached_secret, token, expiry)) = tokens.get(appid) {
            if cached_secret == secret && *expiry > Instant::now() {
                return Ok(token.clone());
            }
        }
        let response = core::json_response(self.client.post(format!("{}/cgi-bin/stable_token", self.base)).json(&json!({"grant_type":"client_credential","appid":appid,"secret":secret,"force_refresh":false})).send().await.map_err(core::network)?).await?;
        let token = response["access_token"]
            .as_str()
            .ok_or_else(|| Error::new("api", "微信响应缺少 access_token"))?
            .to_owned();
        let seconds = response["expires_in"]
            .as_u64()
            .unwrap_or(7200)
            .saturating_sub(120);
        tokens.insert(
            appid.into(),
            (
                secret.into(),
                token.clone(),
                Instant::now() + Duration::from_secs(seconds),
            ),
        );
        Ok(token)
    }
    async fn upload(
        &self,
        source: &str,
        cover: bool,
        token: &str,
        base: Option<&Path>,
    ) -> Result<Value> {
        let asset = transform::load_asset(&self.client, source, base).await?;
        if cover && !matches!(asset.mime.as_str(), "image/jpeg" | "image/png") {
            return Err(Error::new("asset", "微信封面请使用 JPG 或 PNG"));
        }
        let part = reqwest::multipart::Part::bytes(asset.bytes)
            .file_name(asset.name)
            .mime_str(&asset.mime)
            .map_err(|_| Error::new("asset", "图片 MIME 类型无效"))?;
        let path = if cover {
            "material/add_material"
        } else {
            "media/uploadimg"
        };
        let mut request = self
            .client
            .post(format!("{}/cgi-bin/{path}", self.base))
            .query(&[("access_token", token)]);
        if cover {
            request = request.query(&[("type", "image")]);
        }
        core::json_response(
            request
                .multipart(reqwest::multipart::Form::new().part("media", part))
                .send()
                .await
                .map_err(core::network)?,
        )
        .await
    }
}
pub fn compatible_html(html: &str) -> String {
    // Strip links and unsupported markup; keep readable sections and inline styles.
    let tags: std::collections::HashSet<&str> = [
        "p",
        "br",
        "strong",
        "em",
        "blockquote",
        "ul",
        "ol",
        "li",
        "h1",
        "h2",
        "h3",
        "h4",
        "pre",
        "code",
        "img",
        "hr",
        "table",
        "thead",
        "tbody",
        "tr",
        "th",
        "td",
    ]
    .into_iter()
    .collect();
    let clean = ammonia::Builder::default()
        .tags(tags)
        .add_url_schemes(transform::LOCAL_SCHEMES)
        .clean(html)
        .to_string();
    clean
        .replace("<p>", "<p style=\"margin:16px 0;line-height:1.8\">")
        .replace("<img ", "<img style=\"max-width:100%;height:auto\" ")
        .replace(
            "<blockquote>",
            "<blockquote style=\"padding-left:12px;border-left:3px solid #b4c5d1;color:#536575\">",
        )
}
#[async_trait]
impl Publisher for WeChat {
    fn target_identity(&self, cfg: &Config) -> Value {
        json!({"appid":core::field(cfg,"appid")})
    }
    async fn upload_asset(
        &self,
        asset: transform::Asset,
        variant: &str,
        cfg: &Config,
    ) -> Result<RemoteAsset> {
        let cover = variant == "cover";
        if cover && !matches!(asset.mime.as_str(), "image/jpeg" | "image/png") {
            return Err(Error::new("asset", "微信封面请使用 JPG 或 PNG"));
        }
        let token = self.token(cfg).await?;
        let part = reqwest::multipart::Part::bytes(asset.bytes)
            .file_name(asset.name)
            .mime_str(&asset.mime)
            .map_err(|_| Error::new("asset", "图片 MIME 类型无效"))?;
        let path = if cover {
            "material/add_material"
        } else {
            "media/uploadimg"
        };
        let mut request = self
            .client
            .post(format!("{}/cgi-bin/{path}", self.base))
            .query(&[("access_token", token.as_str())]);
        if cover {
            request = request.query(&[("type", "image")]);
        }
        let r = core::json_response(
            request
                .multipart(reqwest::multipart::Form::new().part("media", part))
                .send()
                .await
                .map_err(core::network)?,
        )
        .await?;
        Ok(RemoteAsset {
            id: if cover {
                r["media_id"]
                    .as_str()
                    .ok_or_else(|| Error::new("api", "封面响应缺少 ID"))?
                    .into()
            } else {
                String::new()
            },
            url: if cover {
                r["url"].as_str().unwrap_or("").into()
            } else {
                r["url"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| Error::new("api", "正文图片响应缺少 URL"))?
                    .into()
            },
        })
    }
    fn manifest(&self) -> Manifest {
        manifest(
            "wechat",
            "WeChat Official Account",
            "公众号官方 API · 默认进入草稿箱",
            &["draft", "update", "assets", "html"],
            json!({"appid":{"type":"string","title":"AppID"},"secret":{"type":"string","title":"AppSecret","secret":true}}),
            &["appid", "secret"],
        )
    }
    async fn test_connection(&self, cfg: &Config) -> Result<()> {
        self.tokens
            .lock()
            .await
            .remove(core::required(cfg, "appid")?);
        self.token(cfg).await?;
        Ok(())
    }
    async fn transform(&self, content: &Content, _cfg: &Config) -> Result<Prepared> {
        let mut p = transform::markdown(content);
        p.html = compatible_html(&p.html);
        p.warnings.push(
            "微信不保留 slug、tags；链接会转为文本。必须提供封面；账号需有草稿与素材 API 权限。"
                .into(),
        );
        Ok(p)
    }
    async fn execute(
        &self,
        action: Action,
        mut p: Prepared,
        cfg: &Config,
        base: Option<&Path>,
        remote_id: Option<&str>,
    ) -> Result<Receipt> {
        if !matches!(action, Action::CreateDraft | Action::Update) {
            return Err(Error::unsupported());
        }
        let cover = if p.assets_processed {
            String::new()
        } else {
            p.content
                .cover
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| Error::new("validation", "微信草稿必须提供封面图片"))?
                .to_owned()
        };
        let token = self.token(cfg).await?;
        if !p.assets_processed {
            for source in transform::image_sources(&p.content.body) {
                let result = self.upload(&source, false, &token, base).await?;
                transform::replace_image(
                    &mut p,
                    &source,
                    result["url"]
                        .as_str()
                        .ok_or_else(|| Error::new("api", "微信图片响应缺少 URL"))?,
                );
            }
        }
        let uploaded = if p.assets_processed {
            json!({"media_id":p.cover_asset.as_ref().ok_or_else(||Error::new("validation","微信草稿必须提供封面"))?.id})
        } else {
            self.upload(&cover, true, &token, base).await?
        };
        let thumb = uploaded["media_id"]
            .as_str()
            .ok_or_else(|| Error::new("api", "微信封面响应缺少 media_id"))?;
        let article = json!({"title":p.content.title,"author":p.content.authors.join(", "),"digest":p.content.summary,"content":p.html,"thumb_media_id":thumb,"need_open_comment":0,"only_fans_can_comment":0});
        let (path, payload) = if action == Action::Update {
            (
                "draft/update",
                json!({"media_id":remote_id.ok_or_else(||Error::new("validation","更新草稿需要 media_id"))?,"index":0,"articles":article}),
            )
        } else {
            ("draft/add", json!({"articles":[article]}))
        };
        let response = core::json_response(
            self.client
                .post(format!("{}/cgi-bin/{path}", self.base))
                .query(&[("access_token", token)])
                .json(&payload)
                .send()
                .await
                .map_err(core::network)?,
        )
        .await?;
        Ok(Receipt {
            id: if action == Action::Update {
                remote_id.unwrap_or_default().into()
            } else {
                response["media_id"]
                    .as_str()
                    .ok_or_else(|| Error::new("api", "微信响应缺少草稿 media_id"))?
                    .into()
            },
            url: None,
            status: "draft".into(),
            revision: Some(p.job_id),
        })
    }
}
