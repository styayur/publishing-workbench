use crate::{
    core::{self, Config, Error, Result},
    extensions::manifest,
    publishing::{Action, Manifest, Publisher, Receipt},
    transform::{self, Prepared},
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;
pub struct GenericRest {
    client: reqwest::Client,
}
impl Default for GenericRest {
    fn default() -> Self {
        Self::new()
    }
}
impl GenericRest {
    pub fn new() -> Self {
        Self {
            client: core::client(),
        }
    }
    fn request(&self, cfg: &Config, test: bool) -> Result<reqwest::RequestBuilder> {
        let url = core::endpoint(core::required(cfg, "endpoint")?)?;
        let method = if test {
            reqwest::Method::HEAD
        } else {
            match core::field(cfg, "method") {
                "PUT" => reqwest::Method::PUT,
                "PATCH" => reqwest::Method::PATCH,
                "POST" | "" => reqwest::Method::POST,
                _ => return Err(Error::new("configuration", "发布仅支持 POST、PUT、PATCH")),
            }
        };
        let mut request = self.client.request(method, url);
        let raw = core::field(cfg, "headers");
        if !raw.trim().is_empty() {
            let headers: serde_json::Map<String, Value> = serde_json::from_str(raw)
                .map_err(|_| Error::new("configuration", "Headers 必须为 JSON 对象"))?;
            for (key, value) in headers {
                let name = reqwest::header::HeaderName::from_bytes(key.as_bytes())
                    .map_err(|_| Error::new("configuration", "Header 名称无效"))?;
                let value = reqwest::header::HeaderValue::from_str(
                    value
                        .as_str()
                        .ok_or_else(|| Error::new("configuration", "Header 值必须为字符串"))?,
                )
                .map_err(|_| Error::new("configuration", "Header 值无效"))?;
                request = request.header(name, value);
            }
        }
        let token = core::field(cfg, "token");
        if !token.is_empty() {
            request = request.bearer_auth(token);
        }
        Ok(request)
    }
}
pub fn payload(template: &str, p: &Prepared, action: Action) -> Result<Value> {
    let metadata: serde_json::Map<String, Value> = p
        .content
        .metadata
        .iter()
        .filter(|(k, _)| !k.starts_with("_workbench_"))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let source = json!({"article_id":p.content.article_id,"remote_id":p.remote_id,"job_id":p.job_id,"intent":p.content.metadata.get("_workbench_intent"),"title":p.content.title,"slug":p.content.slug,"summary":p.content.summary,"body":p.content.body,"html":p.html,"cover":p.content.cover,"authors":p.content.authors,"tags":p.content.tags,"metadata":metadata,"action":action});
    fn render(v: &mut Value, source: &Value) -> Result<()> {
        match v {
            Value::String(s) => {
                if let Some(key) = s.strip_prefix("{{").and_then(|s| s.strip_suffix("}}")) {
                    if !key.contains("{{") {
                        *v = source.get(key.trim()).cloned().ok_or_else(|| {
                            Error::new("configuration", format!("未知模板变量 {key}"))
                        })?;
                        return Ok(());
                    }
                }
                let re = regex::Regex::new(r"\{\{\s*(\w+)\s*\}\}").expect("regex");
                let mut output = s.clone();
                for capture in re.captures_iter(s) {
                    let value = source
                        .get(&capture[1])
                        .ok_or_else(|| Error::new("configuration", "未知模板变量"))?;
                    let text = value
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| value.to_string());
                    output = output.replace(&capture[0], &text);
                }
                *s = output;
            }
            Value::Array(a) => {
                for item in a {
                    render(item, source)?;
                }
            }
            Value::Object(o) => {
                for item in o.values_mut() {
                    render(item, source)?;
                }
            }
            _ => {}
        };
        Ok(())
    }
    let mut value: Value = serde_json::from_str(template).map_err(|_| {
        Error::new(
            "configuration",
            "JSON body template 不是有效 JSON；变量请放在字符串中",
        )
    })?;
    render(&mut value, &source)?;
    Ok(value)
}
#[async_trait]
impl Publisher for GenericRest {
    fn target_identity(&self, cfg: &Config) -> Value {
        json!({"endpoint":core::field(cfg,"endpoint")})
    }
    fn safe_remote_retry(&self, cfg: &Config) -> bool {
        core::field(cfg, "idempotency_supported") == "true"
    }
    fn manifest(&self) -> Manifest {
        manifest(
            "generic-rest",
            "Generic REST",
            "自定义 JSON 请求 · HEAD 测试连接",
            &["draft", "publish", "update", "html", "markdown", "tags"],
            json!({
                "endpoint":{"type":"string","title":"Endpoint","placeholder":"https://example.com/articles"},
                "update_endpoint":{"type":"string","title":"Update endpoint（可选，{{remote_id}}）","placeholder":"https://example.com/articles/{{remote_id}}"},
                "update_method":{"type":"string","title":"Update method","enum":["PUT","PATCH","POST"],"default":"PUT"},
                "idempotency_supported":{"type":"string","title":"服务是否保证 Idempotency-Key 幂等","enum":["false","true"],"default":"false"},
                "method":{"type":"string","title":"HTTP Method","enum":["POST","PUT","PATCH"],"default":"POST"},
                "headers":{"type":"string","title":"Headers（JSON）","secret":true,"multiline":true,"default":"{}"},
                "token":{"type":"string","title":"Bearer Token","secret":true},
                "template":{"type":"string","title":"JSON body template","multiline":true,"default":"{\"title\":\"{{title}}\",\"content\":\"{{html}}\",\"tags\":\"{{tags}}\",\"action\":\"{{action}}\"}"}
            }),
            &["endpoint", "method", "template"],
        )
    }
    async fn test_connection(&self, cfg: &Config) -> Result<()> {
        let response = self
            .request(cfg, true)?
            .send()
            .await
            .map_err(core::network)?;
        if !response.status().is_success() {
            return Err(Error::new(
                "connection",
                format!(
                    "HEAD 测试失败 (HTTP {})；请检查认证或服务是否支持 HEAD",
                    response.status().as_u16()
                ),
            ));
        }
        Ok(())
    }
    async fn execute(
        &self,
        action: Action,
        p: Prepared,
        cfg: &Config,
        _base: Option<&Path>,
        remote_id: Option<&str>,
    ) -> Result<Receipt> {
        if action == Action::Delete {
            return Err(Error::unsupported());
        }
        let mut request_cfg = cfg.clone();
        if action == Action::Update {
            let template = core::field(cfg, "update_endpoint");
            if template.is_empty() {
                return Err(Error::new(
                    "unsupported",
                    "请配置 Update endpoint；不会重复创建",
                ));
            }
            let id = remote_id.ok_or_else(|| Error::new("validation", "更新需要 remote_id"))?;
            let encoded =
                percent_encoding::utf8_percent_encode(id, percent_encoding::NON_ALPHANUMERIC)
                    .to_string();
            request_cfg.insert(
                "endpoint".into(),
                json!(template.replace("{{remote_id}}", &encoded)),
            );
            request_cfg.insert(
                "method".into(),
                json!(if core::field(cfg, "update_method").is_empty() {
                    "PUT"
                } else {
                    core::field(cfg, "update_method")
                }),
            );
        }
        if transform::image_sources(&p.content.body)
            .iter()
            .any(|s| !s.starts_with("https://") && !s.starts_with("http://"))
            || p.content.cover.as_deref().is_some_and(|s| {
                !s.is_empty() && !s.starts_with("https://") && !s.starts_with("http://")
            })
        {
            return Err(Error::new(
                "unsupported",
                "Generic REST 不支持素材上传，请使用远程图片 URL",
            ));
        }
        let payload = payload(core::required(cfg, "template")?, &p, action)?;
        let mut request = self.request(&request_cfg, false)?;
        if !p.job_id.is_empty() {
            request = request
                .header("Idempotency-Key", &p.job_id)
                .header("X-Article-Id", &p.content.article_id);
        }
        let response = request.json(&payload).send().await.map_err(core::network)?;
        let status = response.status();
        let revision = response
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        if status == reqwest::StatusCode::NO_CONTENT {
            return Ok(Receipt {
                id: remote_id.unwrap_or_default().into(),
                url: None,
                status: "accepted".into(),
                revision,
            });
        }
        let result = core::json_response(response).await?;
        let id = result
            .get("id")
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| v.to_string())
            })
            .unwrap_or_else(|| remote_id.unwrap_or_default().into());
        Ok(Receipt {
            id,
            url: result["url"].as_str().map(str::to_owned),
            status: "accepted".into(),
            revision,
        })
    }
}
