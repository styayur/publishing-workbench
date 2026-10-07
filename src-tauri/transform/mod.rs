use crate::{
    content::Content,
    core::{Error, Result},
};
use futures_util::StreamExt;
use pulldown_cmark::{html, Options, Parser};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Prepared {
    pub content: Content,
    pub html: String,
    pub warnings: Vec<String>,
    #[serde(default)]
    pub assets_processed: bool,
    #[serde(default)]
    pub cover_asset: Option<crate::publishing::RemoteAsset>,
    #[serde(default)]
    pub job_id: String,
    #[serde(default)]
    pub remote_id: Option<String>,
    #[serde(default)]
    pub asset_records: Vec<crate::publishing::RemoteAsset>,
}
// Windows absolute image paths use a drive-letter scheme in HTML URLs.
pub const LOCAL_SCHEMES: [&str; 27] = [
    "file", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q",
    "r", "s", "t", "u", "v", "w", "x", "y", "z",
];
pub fn markdown(content: &Content) -> Prepared {
    let mut html = String::new();
    html::push_html(
        &mut html,
        Parser::new_ext(
            &content.body,
            Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH,
        ),
    );
    Prepared {
        content: content.clone(),
        html: ammonia::Builder::default()
            .add_url_schemes(LOCAL_SCHEMES)
            .clean(&html)
            .to_string(),
        warnings: vec![],
        assets_processed: false,
        cover_asset: None,
        job_id: String::new(),
        remote_id: None,
        asset_records: vec![],
    }
}
pub fn replace_markdown_image(p: &mut Prepared, source: &str, target: &str) {
    let mut replacements = Vec::new();
    for (event, range) in Parser::new(&p.content.body).into_offset_iter() {
        if let pulldown_cmark::Event::Start(pulldown_cmark::Tag::Image { dest_url, .. }) = event {
            let dest = dest_url.as_ref();
            if dest == source
                || percent_encoding::percent_decode_str(source)
                    .decode_utf8()
                    .is_ok_and(|s| s == dest)
            {
                let slice = &p.content.body[range.clone()];
                replacements.push((range, slice.replacen(dest, target, 1)));
            }
        }
    }
    for (range, value) in replacements.into_iter().rev() {
        p.content.body.replace_range(range, &value);
    }
}
pub fn image_sources(body: &str) -> Vec<String> {
    let mut sources = Vec::new();
    let mut rendered = String::new();
    html::push_html(&mut rendered, Parser::new(body));
    // Match the same normalized, sanitized attributes used by the output HTML.
    let rendered = ammonia::Builder::default()
        .add_url_schemes(LOCAL_SCHEMES)
        .clean(&rendered)
        .to_string();
    let re =
        regex::Regex::new(r#"(?i)<img\b[^>]*?\bsrc\s*=\s*["']([^"']+)["']"#).expect("image regex");
    for capture in re.captures_iter(&rendered) {
        let src = html_escape::decode_html_entities(&capture[1]).to_string();
        if !sources.contains(&src) {
            sources.push(src);
        }
    }
    sources
}
pub fn replace_image(prepared: &mut Prepared, source: &str, target: &str) {
    // Work on HTML attributes, not arbitrary article text or code examples.
    let escaped = |s: &str| {
        s.replace('&', "&amp;")
            .replace('"', "&quot;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    prepared.html = prepared.html.replace(
        &format!("src=\"{}\"", escaped(source)),
        &format!("src=\"{}\"", escaped(target)),
    );
}
#[derive(Debug)]
pub struct Asset {
    pub bytes: Vec<u8>,
    pub name: String,
    pub mime: String,
}
pub async fn load_asset(
    client: &reqwest::Client,
    source: &str,
    base: Option<&Path>,
) -> Result<Asset> {
    const LIMIT: usize = 10 * 1024 * 1024;
    let (bytes, mut name, content_type) =
        if source.starts_with("https://") || source.starts_with("http://") {
            let url = crate::core::endpoint(source)?;
            let response = client.get(url).send().await.map_err(crate::core::network)?;
            if !response.status().is_success() {
                return Err(Error::new(
                    "asset",
                    format!("图片下载失败 (HTTP {})", response.status().as_u16()),
                ));
            }
            if response.content_length().is_some_and(|n| n > LIMIT as u64) {
                return Err(Error::new("asset", "图片超过 10 MB"));
            }
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(|v| v.split(';').next().unwrap_or("").to_owned())
                .filter(|v| {
                    matches!(
                        v.as_str(),
                        "image/jpeg" | "image/png" | "image/gif" | "image/webp"
                    )
                });
            let mut data = Vec::new();
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(crate::core::network)?;
                if data.len() + chunk.len() > LIMIT {
                    return Err(Error::new("asset", "图片超过 10 MB"));
                }
                data.extend(chunk);
            }
            let name = url::Url::parse(source)
                .ok()
                .and_then(|u| {
                    u.path_segments()
                        .and_then(|mut p| p.next_back())
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| "image.png".into());
            (data, name, content_type)
        } else {
            let path = if source.starts_with("file:") {
                url::Url::parse(source)
                    .ok()
                    .and_then(|u| u.to_file_path().ok())
                    .ok_or_else(|| Error::new("asset", "无效的 file URL"))?
            } else {
                Path::new(
                    percent_encoding::percent_decode_str(source)
                        .decode_utf8()
                        .map_err(|_| Error::new("asset", "本地图片路径编码无效"))?
                        .as_ref(),
                )
                .to_path_buf()
            };
            let path = if path.is_absolute() {
                path
            } else {
                base.ok_or_else(|| Error::new("asset", "相对图片路径需要在 Settings 设置图片目录"))?
                    .join(path)
            };
            let size = tokio::fs::metadata(&path)
                .await
                .map_err(|_| Error::new("asset", "本地图片不存在或不可读取"))?
                .len();
            if size > LIMIT as u64 {
                return Err(Error::new("asset", "图片超过 10 MB"));
            }
            let bytes = tokio::fs::read(&path)
                .await
                .map_err(|_| Error::new("asset", "本地图片读取失败"))?;
            (
                bytes,
                path.file_name()
                    .and_then(|v| v.to_str())
                    .unwrap_or("image.png")
                    .to_owned(),
                None,
            )
        };
    let mime = content_type.unwrap_or_else(|| {
        mime_guess::from_path(&name)
            .first_or_octet_stream()
            .to_string()
    });
    if !matches!(
        mime.as_str(),
        "image/jpeg" | "image/png" | "image/gif" | "image/webp"
    ) || bytes.is_empty()
    {
        return Err(Error::new("asset", "仅支持非空 JPG、PNG、GIF、WebP 图片"));
    }
    if mime_guess::from_path(&name).first().is_none() {
        let suffix = match mime.as_str() {
            "image/jpeg" => "jpg",
            "image/png" => "png",
            "image/gif" => "gif",
            _ => "webp",
        };
        name = format!("image.{suffix}");
    }
    Ok(Asset { bytes, name, mime })
}
