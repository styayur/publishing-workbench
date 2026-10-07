use crate::{
    content::Content,
    core::{self, Config, Error, Result},
    extensions::manifest,
    publishing::{Action, Manifest, Publisher, Receipt, RemoteAsset},
    storage,
    transform::{self, Prepared},
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

pub struct GitContent;
fn git(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .map_err(|_| Error::new("git", "无法运行 git，请安装 Git 并加入 PATH"))?;
    if !output.status.success() {
        return Err(Error::new("git",format!("Git {} 失败 (exit {})；检查仓库、用户身份、分支 upstream 和认证。未执行 reset 或强制 push",args.first().unwrap_or(&"operation"),output.status.code().unwrap_or(-1))));
    }
    String::from_utf8(output.stdout).map_err(|_| Error::new("git", "Git 输出不是 UTF-8"))
}
fn repository(cfg: &Config) -> Result<PathBuf> {
    let root = fs::canonicalize(core::required(cfg, "repository_path")?)
        .map_err(|_| Error::new("configuration", "Git 仓库路径不存在"))?;
    let top = git(&root, &["rev-parse", "--show-toplevel"])?;
    let top = fs::canonicalize(top.trim()).map_err(|_| Error::new("git", "无法解析仓库根目录"))?;
    if root != top {
        return Err(Error::new(
            "configuration",
            "repository path 必须是仓库根目录",
        ));
    }
    Ok(root)
}
fn git_dir(root: &Path) -> Result<PathBuf> {
    let dir = git(root, &["rev-parse", "--absolute-git-dir"])?;
    Ok(PathBuf::from(dir.trim()).join("publishing-workbench"))
}
pub fn checked_path(root: &Path, relative: &str) -> Result<PathBuf> {
    let path = Path::new(relative);
    if path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        || relative.is_empty()
        || relative
            .split(['/', '\\'])
            .any(|s| s.eq_ignore_ascii_case(".git"))
    {
        return Err(Error::new("boundary", "内容路径必须为安全的仓库内相对路径"));
    }
    let mut current = root.to_path_buf();
    for c in path.components() {
        current.push(c);
        if current.exists() {
            let meta = fs::symlink_metadata(&current)
                .map_err(|_| Error::new("git", "无法读取目标路径"))?;
            if meta.file_type().is_symlink() {
                return Err(Error::new("boundary", "拒绝符号链接路径"));
            }
            let resolved =
                fs::canonicalize(&current).map_err(|_| Error::new("git", "无法解析路径"))?;
            if !resolved.starts_with(root) {
                return Err(Error::new("boundary", "路径越出仓库"));
            }
        }
    }
    Ok(current)
}
fn option<'a>(cfg: &'a Config, key: &str, default: &'a str) -> &'a str {
    let value = core::field(cfg, key);
    if value.is_empty() {
        default
    } else {
        value
    }
}
fn kind(c: &Content, cfg: &Config) -> Result<&'static str> {
    match c
        .metadata
        .get("content_kind")
        .and_then(Value::as_str)
        .unwrap_or(option(cfg, "content_kind", "Writing"))
    {
        "Writing" => Ok("posts"),
        "Project" => Ok("projects"),
        "Library" => Ok("library"),
        _ => Err(Error::new(
            "validation",
            "content_kind 必须为 Writing、Project 或 Library",
        )),
    }
}
fn status(c: &Content, action: Action, remote: Option<&str>, cfg: &Config) -> Result<&'static str> {
    if action == Action::CreateDraft {
        return Ok("draft");
    }
    if c.metadata.get("visibility").and_then(Value::as_str) == Some("private")
        || c.metadata.get("status").and_then(Value::as_str) == Some("private")
    {
        return Ok("private");
    }
    if action == Action::Update {
        if let Some(remote) = remote {
            if remote.starts_with(option(cfg, "draft_path", "content/drafts")) {
                return Ok("draft");
            }
            if remote.starts_with(option(cfg, "private_path", "content/private")) {
                return Ok("private");
            }
        }
    }
    Ok("published")
}
fn content_directory(cfg: &Config, status: &str, kind: &str) -> String {
    match status {
        "draft" => format!("{}/{}", option(cfg, "draft_path", "content/drafts"), kind),
        "private" => format!(
            "{}/{}",
            option(cfg, "private_path", "content/private"),
            kind
        ),
        _ => option(
            cfg,
            match kind {
                "projects" => "published_projects_path",
                "library" => "published_library_path",
                _ => "published_posts_path",
            },
            match kind {
                "projects" => "content/published/projects",
                "library" => "content/published/library",
                _ => "content/published/posts",
            },
        )
        .into(),
    }
}
fn asset_directory(cfg: &Config, status: &str) -> String {
    match status {
        "draft" => format!("{}/assets", option(cfg, "draft_path", "content/drafts")),
        "private" => format!("{}/assets", option(cfg, "private_path", "content/private")),
        _ => option(cfg, "assets_path", "content/published/assets").into(),
    }
}
pub fn document(c: &Content, cfg: &Config, status: &str) -> Result<String> {
    if !regex::Regex::new(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")
        .expect("slug")
        .is_match(&c.slug)
    {
        return Err(Error::new(
            "validation",
            "Git 内容 slug 必须为小写字母、数字和单连字符",
        ));
    }
    if c.summary.trim().is_empty() {
        return Err(Error::new(
            "validation",
            "Digital Garden description/摘要不能为空",
        ));
    }
    let kind = kind(c, cfg)?;
    let date = c
        .metadata
        .get("date")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());
    if chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").is_err() {
        return Err(Error::new("validation", "date 必须为有效 YYYY-MM-DD"));
    }
    let mut fm = json!({"title":c.title,"slug":c.slug,"date":date,"status":status,"description":c.summary,"tags":c.tags});
    if kind == "posts" {
        let maturity = c
            .metadata
            .get("maturity")
            .and_then(Value::as_str)
            .unwrap_or("seedling");
        if !matches!(maturity, "seedling" | "budding" | "evergreen") {
            return Err(Error::new("validation", "maturity 无效"));
        }
        fm["maturity"] = json!(maturity);
    }
    if kind == "projects" {
        let defaults = json!({"name":c.title,"summary":c.summary,"year":&date[..4],"projectStatus":"experimental","category":"experiments","featured":false,"priority":0,"stack":[],"links":[]});
        for (key, value) in defaults.as_object().expect("object") {
            fm[key] = c
                .metadata
                .get(key)
                .cloned()
                .unwrap_or_else(|| value.clone());
        }
        if !matches!(
            fm["projectStatus"].as_str(),
            Some("stable" | "beta" | "research" | "experimental" | "maintenance")
        ) || !matches!(
            fm["category"].as_str(),
            Some(
                "systems"
                    | "knowledge"
                    | "creative"
                    | "education"
                    | "humanities"
                    | "experiments"
                    | "utilities"
                    | "concepts"
            )
        ) || !fm["featured"].is_boolean()
            || fm["priority"].as_i64().is_none()
            || !fm["stack"].is_array()
            || !fm["links"].is_array()
        {
            return Err(Error::new("validation", "Project metadata 类型或枚举无效"));
        }
        for key in ["github", "live", "release"] {
            if let Some(v) = c.metadata.get(key) {
                let url = v
                    .as_str()
                    .ok_or_else(|| Error::new("validation", "Project URL 必须为字符串"))?;
                core::endpoint(url)?;
                fm[key] = v.clone();
            }
        }
        if let Some(cover) = c
            .cover
            .as_ref()
            .filter(|s| s.starts_with("/garden-assets/"))
        {
            fm["image"] = json!(cover);
        }
    }
    if kind == "library" {
        let k = c
            .metadata
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("note");
        if !matches!(k, "book" | "note" | "quote") {
            return Err(Error::new("validation", "Library kind 无效"));
        }
        fm["kind"] = json!(k);
        if k == "book" {
            let s = c
                .metadata
                .get("readingStatus")
                .and_then(Value::as_str)
                .unwrap_or("queued");
            if !matches!(s, "reading" | "queued" | "read") {
                return Err(Error::new("validation", "readingStatus 无效"));
            }
            fm["readingStatus"] = json!(s);
        }
        for key in ["author", "note"] {
            if let Some(v) = c.metadata.get(key) {
                if !v.is_string() {
                    return Err(Error::new("validation", "Library metadata 必须为字符串"));
                }
                fm[key] = v.clone();
            }
        }
    }
    if option(cfg, "profile", "Digital Garden Engine") == "Generic Markdown" {
        fm["article_id"] = json!(c.article_id);
    }
    let mut header = String::from("---\n");
    for (key, value) in fm.as_object().expect("frontmatter") {
        header.push_str(&format!("{key}: {value}\n"));
    }
    header.push_str("---\n\n");
    header.push_str(&c.body);
    header.push_str(&format!(
        "\n\n<!-- workbench article:{} -->\n",
        c.article_id
    ));
    Ok(header)
}
#[derive(Clone, Serialize, Deserialize)]
struct Change {
    path: String,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
}
#[derive(Serialize, Deserialize)]
struct Transaction {
    changes: Vec<Change>,
    phase: String,
}
fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::new("git", "路径无父目录"))?;
    fs::create_dir_all(parent).map_err(|_| Error::new("git", "无法建立内容目录"))?;
    let temp = path.with_extension(format!("wb-{}.tmp", uuid::Uuid::new_v4()));
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)
        .map_err(|_| Error::new("git", "无法写入临时文件"))?;
    file.write_all(data)
        .and_then(|_| file.sync_all())
        .map_err(|_| Error::new("git", "内容写入失败"))?;
    fs::rename(&temp, path).map_err(|_| Error::new("git", "无法原子替换内容文件"))?;
    Ok(())
}
fn dirty(root: &Path) -> Result<Vec<String>> {
    let s = git(
        root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
    let mut paths = vec![];
    for entry in s.split('\0').filter(|s| !s.is_empty()) {
        if entry.len() < 3 || entry.starts_with('R') || entry.starts_with('C') {
            return Err(Error::new(
                "git_dirty",
                "工作区有重命名或未知修改，请先提交或移开",
            ));
        }
        paths.push(entry[3..].into());
    }
    Ok(paths)
}

#[async_trait]
impl Publisher for GitContent {
    fn resolve_asset_source(
        &self,
        source: &str,
        p: &Prepared,
        action: Action,
        cfg: &Config,
    ) -> Result<String> {
        if let Some(name) = source.strip_prefix("/garden-assets/") {
            let root = repository(cfg)?;
            let status = status(&p.content, action, p.remote_id.as_deref(), cfg)?;
            let path = checked_path(&root, &format!("{}/{}", asset_directory(cfg, status), name))?;
            Ok(path.to_string_lossy().into_owned())
        } else {
            Ok(source.into())
        }
    }
    fn manifest(&self) -> Manifest {
        manifest(
            "git-content",
            "Git Content",
            "本地 Git 内容仓库；保留现有 publication gate",
            &[
                "draft", "publish", "update", "delete", "assets", "markdown", "mdx", "tags",
                "revision", "preview",
            ],
            json!({
                "repository_path":{"type":"string","title":"Repository path"},"profile":{"type":"string","title":"Content profile","enum":["Digital Garden Engine","Generic Markdown"],"default":"Digital Garden Engine"},"content_kind":{"type":"string","title":"Content kind","enum":["Writing","Project","Library"],"default":"Writing"},
                "published_posts_path":{"type":"string","title":"Published posts path","default":"content/published/posts"},"published_projects_path":{"type":"string","title":"Published projects path","default":"content/published/projects"},"published_library_path":{"type":"string","title":"Published library path","default":"content/published/library"},"draft_path":{"type":"string","title":"Draft path","default":"content/drafts"},"private_path":{"type":"string","title":"Private path","default":"content/private"},"assets_path":{"type":"string","title":"Published assets path","default":"content/published/assets"},
                "auto_commit":{"type":"string","title":"Auto commit","enum":["false","true"],"default":"false"},"auto_push":{"type":"string","title":"Auto push（触发现有 CI，默认关闭）","enum":["false","true"],"default":"false"},"commit_template":{"type":"string","title":"Commit template","default":"content: {{operation}} {{slug}}"}
            }),
            &["repository_path", "profile"],
        )
    }
    fn target_identity(&self, cfg: &Config) -> Value {
        json!({"repository_path":fs::canonicalize(core::field(cfg,"repository_path")).unwrap_or_else(|_|PathBuf::from(core::field(cfg,"repository_path"))),"profile":option(cfg,"profile","Digital Garden Engine")})
    }
    fn safe_remote_retry(&self, _cfg: &Config) -> bool {
        true
    }
    fn asset_variant(&self, cover: bool, p: &Prepared, action: Action, cfg: &Config) -> String {
        let _ = cover;
        status(&p.content, action, p.remote_id.as_deref(), cfg)
            .unwrap_or("draft")
            .into()
    }
    async fn test_connection(&self, cfg: &Config) -> Result<()> {
        let root = repository(cfg)?;
        for (key, default) in [
            ("published_posts_path", "content/published/posts"),
            ("published_projects_path", "content/published/projects"),
            ("published_library_path", "content/published/library"),
            ("draft_path", "content/drafts"),
            ("private_path", "content/private"),
            ("assets_path", "content/published/assets"),
        ] {
            checked_path(&root, option(cfg, key, default))?;
        }
        if !dirty(&root)?.is_empty() {
            return Err(Error::new(
                "git_dirty",
                "Git 工作区有未提交修改；工作台不会覆盖它们，请先提交或使用干净的专用 checkout",
            ));
        }
        if core::field(cfg, "auto_push") == "true" && core::field(cfg, "auto_commit") != "true" {
            return Err(Error::new(
                "configuration",
                "auto push 需要启用 auto commit",
            ));
        }
        Ok(())
    }
    async fn transform(&self, c: &Content, cfg: &Config) -> Result<Prepared> {
        let mut p = transform::markdown(c);
        document(c, cfg, "draft")?;
        p.warnings.push("Git 发布只写内容仓库；部署由仓库已有 GitHub Actions 验证与 publication gate 完成。草稿不会进入 published。".into());
        Ok(p)
    }
    async fn upload_asset(
        &self,
        asset: transform::Asset,
        variant: &str,
        cfg: &Config,
    ) -> Result<RemoteAsset> {
        let root = repository(cfg)?;
        let hash = storage::hash(&asset.bytes);
        let ext = Path::new(&asset.name)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("png");
        let filename = format!("{hash}.{ext}");
        let stage = git_dir(&root)?.join("assets").join(&filename);
        if stage.exists() {
            if fs::read(&stage).map_err(|_| Error::new("git", "无法读取素材缓存"))? != asset.bytes
            {
                return Err(Error::new("git", "素材缓存损坏"));
            }
        } else {
            atomic_write(&stage, &asset.bytes)?;
        }
        Ok(RemoteAsset {
            id: format!("{}/{}", asset_directory(cfg, variant), filename),
            url: format!("/garden-assets/{filename}"),
        })
    }
    async fn execute(
        &self,
        action: Action,
        mut p: Prepared,
        cfg: &Config,
        _base: Option<&Path>,
        remote_id: Option<&str>,
    ) -> Result<Receipt> {
        let root = repository(cfg)?;
        let intent: Action = p
            .content
            .metadata
            .get("_workbench_intent")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or(action);
        let status = status(&p.content, intent, remote_id, cfg)?;
        let k = kind(&p.content, cfg)?;
        let ext = if p.content.format == "mdx" {
            "mdx"
        } else {
            "md"
        };
        let relative = format!(
            "{}/{}.{}",
            content_directory(cfg, status, k),
            p.content.slug,
            ext
        );
        let path = checked_path(&root, &relative)?;
        let draft = checked_path(&root, option(cfg, "draft_path", "content/drafts"))?;
        let private = checked_path(&root, option(cfg, "private_path", "content/private"))?;
        for k in ["posts", "projects", "library"] {
            let published = checked_path(&root, &content_directory(cfg, "published", k))?;
            if draft.starts_with(&published)
                || published.starts_with(&draft)
                || private.starts_with(&published)
                || published.starts_with(&private)
                || private.starts_with(&draft)
                || draft.starts_with(&private)
            {
                return Err(Error::new(
                    "boundary",
                    "Draft、private 与 published 路径必须互不重叠",
                ));
            }
        }

        if option(cfg, "profile", "Digital Garden Engine") == "Digital Garden Engine" {
            let expected = format!(
                "content/{}/{k}",
                match status {
                    "draft" => "drafts",
                    "private" => "private",
                    _ => "published",
                }
            );
            if content_directory(cfg, status, k) != expected {
                return Err(Error::new("boundary","Digital Garden profile 路径必须匹配 content/{published,drafts,private}/{posts,projects,library}"));
            }
            if asset_directory(cfg, status)
                != format!(
                    "content/{}/assets",
                    match status {
                        "draft" => "drafts",
                        "private" => "private",
                        _ => "published",
                    }
                )
            {
                return Err(Error::new(
                    "boundary",
                    "Digital Garden 素材目录必须与内容 status 一致",
                ));
            }
        }
        if action != Action::Delete
            && p.content
                .cover
                .as_ref()
                .is_some_and(|s| !s.starts_with("/garden-assets/"))
        {
            return Err(Error::new(
                "asset",
                "Git 素材需通过可靠发布链复制到内容仓库",
            ));
        }
        let body = document(&p.content, cfg, status)?;
        let after = if action == Action::Delete {
            None
        } else {
            Some(body.into_bytes())
        };
        let job = if p.job_id.is_empty() {
            uuid::Uuid::new_v4().to_string()
        } else {
            p.job_id.clone()
        };
        if uuid::Uuid::parse_str(&job).is_err() {
            return Err(Error::new("validation", "job_id 无效"));
        }
        let tx_path = git_dir(&root)?
            .join("transactions")
            .join(format!("{job}.json"));
        let mut tx: Transaction = if tx_path.exists() {
            serde_json::from_slice(
                &fs::read(&tx_path).map_err(|_| Error::new("git", "无法读取 Git 事务"))?,
            )
            .map_err(|_| Error::new("git", "Git 事务格式无效"))?
        } else {
            let indexed = git(&root, &["diff", "--cached", "--name-only", "-z"])?;
            if !indexed.is_empty() {
                return Err(Error::new(
                    "git_dirty",
                    "Index 有用户暂存修改，未写入；请先提交",
                ));
            }
            let dirty_paths = dirty(&root)?;
            let mut allowed = BTreeSet::new();
            if let Some(old) = remote_id {
                if let Some(rev) = p
                    .content
                    .metadata
                    .get("_workbench_revision")
                    .and_then(Value::as_str)
                {
                    let file = checked_path(&root, old)?;
                    if fs::read(file)
                        .is_ok_and(|b| rev.rsplit(':').next() == Some(storage::hash(&b).as_str()))
                    {
                        allowed.insert(old.to_owned());
                    }
                }
            }
            for a in &p.asset_records {
                let file = checked_path(&root, &a.id)?;
                let stage = git_dir(&root)?
                    .join("assets")
                    .join(file.file_name().unwrap_or_default());
                if let (Ok(current), Ok(expected)) = (fs::read(file), fs::read(stage)) {
                    if current == expected {
                        allowed.insert(a.id.clone());
                    }
                }
            }
            if dirty_paths.iter().any(|path| !allowed.contains(path)) {
                return Err(Error::new(
                    "git_dirty",
                    "工作区有未提交修改，未写入任何文章；请先提交或使用专用 checkout",
                ));
            }
            let mut changes = vec![];
            if let Some(old) = remote_id.filter(|s| !s.is_empty()) {
                let old_path = checked_path(&root, old)?;
                let existing = fs::read(&old_path)
                    .map_err(|_| Error::new("git_conflict", "映射文件不存在，请核对远端映射"))?;
                let marker = format!("workbench article:{}", p.content.article_id);
                if !String::from_utf8_lossy(&existing).contains(&marker) {
                    return Err(Error::new("git_conflict", "映射文件不属于该文章，未覆盖"));
                }
                if let Some(revision) = p
                    .content
                    .metadata
                    .get("_workbench_revision")
                    .and_then(Value::as_str)
                {
                    if revision.rsplit(':').next() != Some(storage::hash(&existing).as_str()) {
                        return Err(Error::new(
                            "git_conflict",
                            "远端文件已被其他工具修改，请核对后重新绑定映射",
                        ));
                    }
                }
                if old != relative || action == Action::Delete {
                    changes.push(Change {
                        path: old.into(),
                        before: Some(existing),
                        after: None,
                    });
                }
            }
            if action != Action::Delete {
                let before = if path.exists() {
                    Some(fs::read(&path).map_err(|_| Error::new("git", "无法读取内容文件"))?)
                } else {
                    None
                };
                if before.is_some() && remote_id != Some(relative.as_str()) {
                    return Err(Error::new(
                        "git_conflict",
                        "目标 slug 已存在但无此文章映射，未覆盖",
                    ));
                }
                changes.push(Change {
                    path: relative.clone(),
                    before,
                    after: after.clone(),
                });
            }
            for asset in &p.asset_records {
                let dest = checked_path(&root, &asset.id)?;
                if !asset.id.starts_with(&(asset_directory(cfg, status) + "/")) {
                    return Err(Error::new("boundary", "素材目标越过 publication boundary"));
                }
                let name = dest
                    .file_name()
                    .ok_or_else(|| Error::new("asset", "素材路径无效"))?;
                let bytes = fs::read(git_dir(&root)?.join("assets").join(name))
                    .map_err(|_| Error::new("asset", "Git 素材缓存缺失"))?;
                let before = fs::read(&dest).ok();
                if before.as_ref().is_some_and(|b| b != &bytes) {
                    return Err(Error::new("git_conflict", "同名素材内容不匹配"));
                }
                if !changes.iter().any(|c| c.path == asset.id) {
                    changes.push(Change {
                        path: asset.id.clone(),
                        before,
                        after: Some(bytes),
                    });
                }
            }
            let tx = Transaction {
                changes,
                phase: "prepared".into(),
            };
            atomic_write(
                &tx_path,
                &serde_json::to_vec(&tx).map_err(|_| Error::new("git", "事务编码失败"))?,
            )?;
            tx
        };
        let owned: BTreeSet<_> = tx.changes.iter().map(|c| c.path.clone()).collect();
        for dirty in dirty(&root)? {
            if !owned.contains(&dirty) {
                return Err(Error::new("git_dirty", "重试时发现无关工作区修改，已停止"));
            }
        }
        for change in &tx.changes {
            let file = checked_path(&root, &change.path)?;
            let current = fs::read(&file).ok();
            if current != change.before && current != change.after {
                return Err(Error::new(
                    "git_conflict",
                    "文件在事务后被用户修改，已停止并保留备份",
                ));
            }
            if current == change.after {
                continue;
            }
            if let Some(bytes) = &change.after {
                atomic_write(&file, bytes)?;
            } else if file.exists() {
                fs::remove_file(&file)
                    .map_err(|_| Error::new("git_pending", "无法删除已核对的映射文件；可重试"))?;
            }
        }
        tx.phase = "written".into();
        atomic_write(
            &tx_path,
            &serde_json::to_vec(&tx).map_err(|_| Error::new("git_pending", "无法保存事务"))?,
        )?;
        let mut revision = String::new();
        if core::field(cfg, "auto_commit") == "true" {
            let changed = dirty(&root)?;
            if !changed.is_empty() {
                let mut args = vec!["add", "--"];
                for f in &owned {
                    args.push(f);
                }
                git(&root, &args).map_err(|e| Error::new("git_pending", e.message))?;
                let message = option(cfg, "commit_template", "content: {{operation}} {{slug}}")
                    .replace(
                        "{{operation}}",
                        if action == Action::Update {
                            "update"
                        } else if action == Action::Delete {
                            "delete"
                        } else {
                            "create"
                        },
                    )
                    .replace("{{slug}}", &p.content.slug)
                    .replace("{{title}}", &p.content.title);
                let message = format!("{message}\n\n[workbench:{job}]");
                let mut args = vec!["commit", "--only", "-m", &message, "--"];
                for f in &owned {
                    args.push(f);
                }
                git(&root, &args).map_err(|e| Error::new("git_pending", e.message))?;
            }
            revision = git(&root, &["rev-parse", "HEAD"])?;
            if core::field(cfg, "auto_push") == "true" {
                git(&root, &["push"]).map_err(|e| {
                    Error::new(
                        "git_pending",
                        format!(
                            "提交已保存，但 push 失败；重试仅恢复提交/推送。{}",
                            e.message
                        ),
                    )
                })?;
            }
        } else if core::field(cfg, "auto_push") == "true" {
            return Err(Error::new("configuration", "auto push 需要 auto commit"));
        }
        tx.phase = "complete".into();
        atomic_write(
            &tx_path,
            &serde_json::to_vec(&tx)
                .map_err(|_| Error::new("git_pending", "无法保存事务完成状态"))?,
        )?;
        let hash = after.as_ref().map(|b| storage::hash(b)).unwrap_or_default();
        p.assets_processed = true;
        Ok(Receipt {
            id: if action == Action::Delete {
                remote_id.unwrap_or(&relative).into()
            } else {
                relative
            },
            url: None,
            status: if action == Action::Delete {
                "deleted"
            } else {
                status
            }
            .into(),
            revision: Some(format!("{}:{hash}", revision.trim())),
        })
    }
}
pub fn detect_gardens(parent: &Path) -> Vec<Value> {
    let mut found = vec![];
    let site_re = regex::Regex::new(r#"https://[a-zA-Z0-9.-]+"#).expect("site URL");
    if let Ok(entries) = fs::read_dir(parent) {
        for e in entries.flatten() {
            let root = e.path();
            if !root.join("engine.lock.json").is_file()
                || !root.join("content/published").is_dir()
                || !root.join("content/drafts").is_dir()
            {
                continue;
            }
            let lock = fs::read_to_string(root.join("engine.lock.json"))
                .ok()
                .and_then(|s| serde_json::from_str::<Value>(&s).ok());
            if lock.as_ref().and_then(|v| v["repository"].as_str())
                != Some("styayur/digital-garden-engine")
            {
                continue;
            }
            let site = fs::read_to_string(root.join("site.config.ts")).unwrap_or_default();
            let site = site_re.find(&site).map(|m| m.as_str().to_owned());
            found.push(json!({"extension_id":"git-content","label":site.unwrap_or_else(||e.file_name().to_string_lossy().into_owned()),"config":{"repository_path":root.to_string_lossy(),"profile":"Digital Garden Engine","auto_commit":"false","auto_push":"false"},"evidence":"engine.lock.json + content/published + content/drafts + site.config.ts"}));
        }
    }
    found
}
