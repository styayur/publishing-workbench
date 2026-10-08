use crate::{
    content::Content,
    core::{Config, Error, Result},
    publishing::{reliable::ReliablePublishing, Action, Manifest, Receipt},
    storage::{PublishJob, Store},
    transform::Prepared,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tauri::Manager;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Workspace {
    pub content: Content,
    pub asset_directory: String,
    pub extensions: BTreeMap<String, Config>,
    pub history: Vec<History>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct History {
    pub target: String,
    pub action: Action,
    pub receipt: Option<Receipt>,
    pub error: Option<Error>,
    pub timestamp: u64,
}
struct State {
    reliable: ReliablePublishing,
    store: Arc<Store>,
    workspace: Mutex<Workspace>,
}
impl State {
    fn persist(&self, workspace: &Workspace) -> Result<()> {
        self.store.save_article(&workspace.content)?;
        let raw = serde_json::to_string(workspace)
            .map_err(|_| Error::new("storage", "无法序列化本地状态"))?;
        self.store.set_setting("workspace", &raw)
    }
    fn config(&self, id: &str) -> Result<Config> {
        let manifest = self.reliable.registry.get(id)?.manifest();
        let mut cfg = self
            .workspace
            .lock()
            .map_err(|_| Error::new("storage", "本地状态不可用"))?
            .extensions
            .get(id)
            .cloned()
            .unwrap_or_default();
        for (name, schema) in properties(&manifest) {
            if schema["secret"] == true {
                let entry = credential(id, &name)?;
                match entry.get_password() {
                    Ok(secret) => {
                        cfg.insert(name, Value::String(secret));
                    }
                    Err(keyring::Error::NoEntry) => {}
                    Err(_) => return Err(Error::new("credentials", "无法读取系统凭据库")),
                }
            } else if !cfg.contains_key(&name) {
                if let Some(default) = schema.get("default") {
                    cfg.insert(name, default.clone());
                }
            }
        }
        Ok(cfg)
    }
}
fn properties(manifest: &Manifest) -> Config {
    manifest.schema["properties"]
        .as_object()
        .cloned()
        .unwrap_or_default()
}
fn credential(id: &str, name: &str) -> Result<keyring::Entry> {
    keyring::Entry::new("publishing-workbench", &format!("{id}:{name}"))
        .map_err(|_| Error::new("credentials", "系统凭据库不可用"))
}

#[tauri::command]
fn manifests(state: tauri::State<'_, State>) -> Vec<Manifest> {
    state.reliable.registry.manifests()
}
#[tauri::command]
fn load_workspace(state: tauri::State<'_, State>) -> Result<Workspace> {
    state
        .workspace
        .lock()
        .map(|v| v.clone())
        .map_err(|_| Error::new("storage", "本地状态不可用"))
}
#[tauri::command]
fn save_content(
    mut content: Content,
    asset_directory: String,
    state: tauri::State<'_, State>,
) -> Result<()> {
    let mut ws = state
        .workspace
        .lock()
        .map_err(|_| Error::new("storage", "本地状态不可用"))?;
    content.ensure_id();
    ws.content = content;
    ws.asset_directory = asset_directory;
    state.persist(&ws)
}
#[tauri::command]
fn import_file(
    path: String,
    mut content: Content,
    state: tauri::State<'_, State>,
) -> Result<Content> {
    let source = std::fs::canonicalize(path).map_err(|_| Error::new("import", "源文件不存在"))?;
    let raw = std::fs::read_to_string(&source)
        .map_err(|_| Error::new("import", "无法读取 UTF-8 文件"))?;
    if raw.len() > 2_000_000 {
        return Err(Error::new("import", "文件超过 2 MB"));
    }
    // Frontmatter is parsed in the GUI; check its source before binding a stable identity.
    let source = source.to_string_lossy().into_owned();
    if let Some(id) = state.store.source_article_id(&source)? {
        content.article_id = id;
    }
    content.ensure_id();
    content.source_path = Some(source);
    let original = std::fs::read(content.source_path.as_ref().expect("import source"))
        .map_err(|_| Error::new("import", "无法校验导入源文件"))?;
    content.metadata.insert(
        "_workbench_import_hash".into(),
        serde_json::json!(crate::storage::hash(&original)),
    );
    content.metadata.remove("_workbench_adopt_existing");
    state.store.save_article(&content)?;
    Ok(content)
}
#[tauri::command]
fn read_content_file(path: String) -> Result<String> {
    let metadata = std::fs::metadata(&path).map_err(|_| Error::new("import", "源文件不存在"))?;
    if !metadata.is_file() || metadata.len() > 2_000_000 {
        return Err(Error::new("import", "只支持 2 MB 以内的文件"));
    }
    std::fs::read_to_string(path).map_err(|_| Error::new("import", "无法读取 UTF-8 文件"))
}
#[tauri::command]
fn save_config(
    id: String,
    config: Config,
    clear_secrets: Vec<String>,
    state: tauri::State<'_, State>,
) -> Result<()> {
    let manifest = state.reliable.registry.get(&id)?.manifest();
    let props = properties(&manifest);
    let mut merged = state.config(&id)?;
    for (name, value) in &config {
        let schema = props
            .get(name)
            .ok_or_else(|| Error::new("configuration", "未知配置项"))?;
        if !value.is_string() {
            return Err(Error::new("configuration", "配置值必须为字符串"));
        }
        if schema["secret"] == true && value.as_str() == Some("") {
            continue;
        }
        if schema["secret"] == true
            && value
                .as_str()
                .is_some_and(|s| s.encode_utf16().count() * 2 > 2400)
        {
            return Err(Error::new(
                "configuration",
                "密钥字段超过系统凭据库长度限制 (1200 UTF-16 units)",
            ));
        }
        merged.insert(name.clone(), value.clone());
    }
    for name in &clear_secrets {
        if props.get(name).is_none_or(|s| s["secret"] != true) {
            return Err(Error::new("configuration", "只能清除密钥字段"));
        }
        merged.remove(name);
    }
    for (name, schema) in props {
        if schema["secret"] == true {
            if clear_secrets.contains(&name) {
                match credential(&id, &name)?.delete_credential() {
                    Ok(()) | Err(keyring::Error::NoEntry) => {}
                    Err(_) => return Err(Error::new("credentials", "无法删除系统凭据")),
                }
            } else if let Some(secret) = config
                .get(&name)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                credential(&id, &name)?
                    .set_password(secret)
                    .map_err(|_| Error::new("credentials", "无法保存系统凭据；配置未保存"))?;
            }
            merged.remove(&name);
        }
    }
    let mut ws = state
        .workspace
        .lock()
        .map_err(|_| Error::new("storage", "本地状态不可用"))?;
    let publisher = state.reliable.registry.get(&id)?;
    let target = ReliablePublishing::target_id(publisher.as_ref(), &merged);
    state
        .store
        .save_target(&target, &id, &serde_json::json!(merged))?;
    ws.extensions.insert(id, merged);
    state.persist(&ws)
}
#[tauri::command]
async fn test_connection(id: String, state: tauri::State<'_, State>) -> Result<()> {
    let cfg = state.config(&id)?;
    state.reliable.registry.get(&id)?.connect(&cfg).await
}
#[tauri::command]
async fn preview(
    content: Content,
    id: Option<String>,
    state: tauri::State<'_, State>,
) -> Result<Prepared> {
    let cfg = id
        .as_deref()
        .map(|id| state.config(id))
        .transpose()?
        .unwrap_or_default();
    let publisher = id
        .as_deref()
        .map(|id| state.reliable.registry.get(id))
        .transpose()?;
    let mut p = if let Some(publisher) = &publisher {
        publisher.preview(&content, &cfg).await?
    } else {
        crate::transform::markdown(&content)
    };
    let mut directory = state
        .workspace
        .lock()
        .map_err(|_| Error::new("storage", "本地状态不可用"))?
        .asset_directory
        .clone();
    if directory.is_empty() {
        directory = content
            .source_path
            .as_deref()
            .and_then(|p| std::path::Path::new(p).parent())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
    }
    let preview_action = if let Some(publisher) = &publisher {
        let target = ReliablePublishing::target_id(publisher.as_ref(), &cfg);
        if let Some(m) = state
            .store
            .mapping(&content.article_id, &target)?
            .filter(|m| m.status != "deleted")
        {
            p.remote_id = Some(m.remote_id);
            Action::Update
        } else if matches!(
            content.metadata.get("status").and_then(Value::as_str),
            Some("published" | "private")
        ) {
            Action::Publish
        } else {
            Action::CreateDraft
        }
    } else {
        Action::CreateDraft
    };
    for source in crate::transform::image_sources(&content.body) {
        if source.starts_with("https://")
            || source.starts_with("http://")
            || source.starts_with("data:")
        {
            continue;
        }
        let resolved = if let Some(publisher) = &publisher {
            publisher.resolve_asset_source(&source, &p, preview_action, &cfg)?
        } else {
            source.clone()
        };
        match crate::transform::load_asset(
            &crate::core::client(),
            &resolved,
            if directory.is_empty() {
                None
            } else {
                Some(std::path::Path::new(&directory))
            },
        )
        .await
        {
            Ok(asset) => {
                use base64::Engine;
                let url = format!(
                    "data:{};base64,{}",
                    asset.mime,
                    base64::engine::general_purpose::STANDARD.encode(asset.bytes)
                );
                crate::transform::replace_image(&mut p, &source, &url);
            }
            Err(e) => p.warnings.push(e.message),
        }
    }
    Ok(p)
}
#[tauri::command]
async fn dispatch(
    id: String,
    action: Action,
    mut content: Content,
    state: tauri::State<'_, State>,
) -> Result<PublishJob> {
    content.ensure_id();
    let cfg = state.config(&id)?;
    let mut directory = state
        .workspace
        .lock()
        .map_err(|_| Error::new("storage", "本地状态不可用"))?
        .asset_directory
        .clone();
    if directory.is_empty() {
        directory = content
            .source_path
            .as_deref()
            .and_then(|p| std::path::Path::new(p).parent())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
    }
    state
        .reliable
        .start(
            &id,
            action,
            content,
            &cfg,
            if directory.is_empty() {
                None
            } else {
                Some(std::path::Path::new(&directory))
            },
        )
        .await
}
#[tauri::command]
async fn retry_job(job_id: String, state: tauri::State<'_, State>) -> Result<PublishJob> {
    let job = state.store.job(&job_id)?;
    let cfg = state.config(&job.extension_id)?;
    state.reliable.retry(&job_id, &cfg).await
}
#[tauri::command]
fn reconcile_job(
    job_id: String,
    remote_id: String,
    remote_status: String,
    state: tauri::State<'_, State>,
) -> Result<()> {
    state.reliable.reconcile(
        &job_id,
        Receipt {
            id: remote_id,
            url: None,
            status: remote_status,
            revision: None,
        },
    )
}
#[tauri::command]
fn cancel_job(job_id: String, state: tauri::State<'_, State>) -> Result<()> {
    state.reliable.cancel(&job_id)
}
#[tauri::command]
fn reconcile_asset(
    asset_hash: String,
    target_id: String,
    variant: String,
    remote_asset_id: String,
    remote_url: String,
    state: tauri::State<'_, State>,
) -> Result<()> {
    let current = state
        .store
        .asset(&asset_hash, &target_id, &variant)?
        .ok_or_else(|| Error::new("validation", "未知素材"))?;
    if !matches!(current.0.as_str(), "pending" | "uncertain") {
        return Err(Error::new("validation", "该素材不需要核对"));
    }
    if remote_asset_id.is_empty() && remote_url.is_empty() {
        return Err(Error::new("validation", "请填写核对后的素材 ID 或 URL"));
    }
    state.store.save_asset(
        &asset_hash,
        &target_id,
        &variant,
        "reconciled",
        "available",
        &crate::publishing::RemoteAsset {
            id: remote_asset_id,
            url: remote_url,
        },
    )
}
#[tauri::command]
fn publication_state(article_id: String, state: tauri::State<'_, State>) -> Result<Value> {
    let mappings = state.store.mappings(&article_id)?;
    let mut plans = vec![];
    for manifest in state.reliable.registry.manifests() {
        let cfg = state.config(&manifest.id)?;
        let p = state.reliable.registry.get(&manifest.id)?;
        let target_id = ReliablePublishing::target_id(p.as_ref(), &cfg);
        let m = mappings
            .iter()
            .find(|m| m.target_id == target_id && m.status != "deleted");
        plans.push(serde_json::json!({"extension_id":manifest.id,"target_id":target_id,"operation":if m.is_some(){"update"}else{"create"},"remote_id":m.map(|m|&m.remote_id)}));
    }
    Ok(
        serde_json::json!({"jobs":state.store.jobs()?,"mappings":mappings,"plans":plans,"orphans":state.store.orphans()?}),
    )
}
#[tauri::command]
fn detect_gardens() -> Vec<Value> {
    let mut roots = std::collections::BTreeSet::new();
    if let Ok(cwd) = std::env::current_dir() {
        roots.insert(cwd.clone());
        if let Some(p) = cwd.parent() {
            roots.insert(p.to_path_buf());
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        for parent in exe.ancestors().take(6) {
            roots.insert(parent.to_path_buf());
        }
    }
    let mut found = std::collections::BTreeMap::new();
    for root in roots {
        for candidate in crate::extensions::git_content::detect_gardens(&root) {
            found.insert(
                candidate["config"]["repository_path"].to_string(),
                candidate,
            );
        }
    }
    found.into_values().collect()
}
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let directory = std::env::var_os("WORKBENCH_DATA_DIR")
                .map(std::path::PathBuf::from)
                .unwrap_or(app.path().app_data_dir()?);
            std::fs::create_dir_all(&directory)?;
            let store = Arc::new(Store::open(directory.join("workbench.sqlite3"))?);
            let raw = match store.setting("workspace")? {
                Some(raw) => Some(raw),
                None => {
                    let legacy = directory.join("workspace.json");
                    if legacy.exists() {
                        Some(std::fs::read_to_string(legacy)?)
                    } else {
                        None
                    }
                }
            };
            let mut workspace: Workspace = match raw {
                Some(raw) => serde_json::from_str(&raw)?,
                None => Workspace::default(),
            };
            workspace.content.ensure_id();
            let state = State {
                reliable: ReliablePublishing::new(store.clone()),
                store,
                workspace: Mutex::new(workspace.clone()),
            };
            state.persist(&workspace)?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            manifests,
            load_workspace,
            save_content,
            save_config,
            import_file,
            read_content_file,
            test_connection,
            preview,
            dispatch,
            retry_job,
            reconcile_job,
            cancel_job,
            reconcile_asset,
            publication_state,
            detect_gardens
        ])
        .run(tauri::generate_context!())
        .expect("启动 Publishing Workbench 失败");
}
