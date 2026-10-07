use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

pub type Result<T> = std::result::Result<T, Error>;
pub type Config = serde_json::Map<String, Value>;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Error {
    pub code: String,
    pub message: String,
}
impl Error {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
    pub fn unsupported() -> Self {
        Self::new("unsupported", "该扩展不支持此操作")
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Error {}
pub fn required<'a>(cfg: &'a Config, key: &str) -> Result<&'a str> {
    cfg.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| Error::new("configuration", format!("请填写配置项 {key}")))
}
pub fn field<'a>(cfg: &'a Config, key: &str) -> &'a str {
    cfg.get(key).and_then(Value::as_str).unwrap_or("")
}
pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(45))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("HTTP client")
}
pub fn network(_: reqwest::Error) -> Error {
    // Never forward reqwest errors: their URLs can contain access tokens.
    Error::new(
        "network",
        "网络请求失败，请检查地址、网络、证书和超时；未自动重试，避免重复发布",
    )
}
pub fn endpoint(value: &str) -> Result<String> {
    let url = url::Url::parse(value).map_err(|_| Error::new("configuration", "请输入有效 URL"))?;
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if url.scheme() != "https" && !(url.scheme() == "http" && local) {
        return Err(Error::new(
            "configuration",
            "远程 API 必须使用 HTTPS；HTTP 仅允许本机 mock",
        ));
    }
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err(Error::new(
            "configuration",
            "URL 不允许包含用户名、密码或片段",
        ));
    }
    Ok(value.trim_end_matches('/').to_owned())
}
pub async fn json_response(response: reqwest::Response) -> Result<Value> {
    let status = response.status();
    if !status.is_success() {
        let (code, message) = match status.as_u16() {
            401 | 403 => (
                "authentication",
                "认证或权限失败，请检查凭据与账号 API 权限",
            ),
            429 => ("rate_limit", "API 请求已限流，请稍后重试"),
            _ => ("api", "API 请求失败，请检查目标配置和服务状态"),
        };
        return Err(Error::new(
            code,
            format!("{message} (HTTP {})", status.as_u16()),
        ));
    }
    let value: Value = response
        .json()
        .await
        .map_err(|_| Error::new("api", "API 未返回有效 JSON"))?;
    if let Some(code) = value
        .get("errcode")
        .and_then(Value::as_i64)
        .filter(|c| *c != 0)
    {
        let (kind, msg) = match code {
            40001 | 40013 | 40125 | 42001 => (
                "authentication",
                "微信凭据无效或 token 已失效，请重新测试连接",
            ),
            45009 | 45011 => ("rate_limit", "微信 API 配额或频率限制，请稍后重试"),
            40164 => ("authentication", "请在公众号后台配置当前公网 IP 白名单"),
            48001 => ("unsupported", "当前公众号没有该 API 权限"),
            _ => ("api", "微信 API 拒绝请求，请按错误码检查素材及账号权限"),
        };
        return Err(Error::new(kind, format!("{msg} (errcode {code})")));
    }
    Ok(value)
}
