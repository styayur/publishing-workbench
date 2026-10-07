use super::{Action, Publisher, Receipt, Registry, RemoteAsset};
use crate::{
    content::Content,
    core::{Config, Error, Result},
    storage::{self, AssetInput, Mapping, PublishJob, Step, Store},
    transform,
};
use serde_json::json;
use std::{path::Path, sync::Arc};

fn public_config(p: &dyn Publisher, cfg: &Config) -> serde_json::Value {
    let mut value = cfg.clone();
    for (k, s) in p.manifest().schema["properties"]
        .as_object()
        .into_iter()
        .flatten()
    {
        if s["secret"] == true {
            value.remove(k);
        }
    }
    json!(value)
}
pub struct ReliablePublishing {
    pub registry: Registry,
    pub store: Arc<Store>,
    gate: tokio::sync::Mutex<()>,
}
impl ReliablePublishing {
    pub fn new(store: Arc<Store>) -> Self {
        Self::with_registry(store, Registry::builtin())
    }
    pub fn with_registry(store: Arc<Store>, registry: Registry) -> Self {
        Self {
            registry,
            store,
            gate: tokio::sync::Mutex::new(()),
        }
    }
    pub fn target_id(p: &dyn Publisher, cfg: &Config) -> String {
        format!(
            "{}:{}",
            p.manifest().id,
            &storage::hash(p.target_identity(cfg).to_string().as_bytes())[..20]
        )
    }
    pub async fn start(
        &self,
        extension: &str,
        action: Action,
        mut c: Content,
        cfg: &Config,
        base: Option<&Path>,
    ) -> Result<PublishJob> {
        let _guard = self.gate.lock().await;
        c.ensure_id();
        c.validate()?;
        let p = self.registry.get(extension)?;
        if !p
            .capabilities()
            .iter()
            .any(|cap| cap == action.capability())
        {
            return Err(Error::unsupported());
        }
        let target = Self::target_id(p.as_ref(), cfg);
        if let Some(job) = self.store.pending(&c.article_id, &target)? {
            return Err(Error::new(
                "pending_job",
                format!(
                    "存在未完成任务 {}，请在 Publish History 重试或核对远端",
                    job.job_id
                ),
            ));
        }
        self.store.save_article(&c)?;
        self.store
            .save_target(&target, extension, &p.target_identity(cfg))?;
        let mapping = self
            .store
            .mapping(&c.article_id, &target)?
            .filter(|m| m.status != "deleted");
        if action == Action::Delete && mapping.is_none() {
            return Err(Error::new("validation", "尚无远端映射，不能删除"));
        }
        if action == Action::Update && mapping.is_none() {
            return Err(Error::new("validation", "尚无远端映射，请先创建草稿或发布"));
        }
        let job = PublishJob {
            job_id: uuid::Uuid::new_v4().to_string(),
            article_id: c.article_id.clone(),
            target_id: target,
            extension_id: extension.into(),
            operation: if action == Action::Delete {
                "delete"
            } else if mapping.is_some() {
                "update"
            } else {
                "create"
            }
            .into(),
            action,
            status: "running".into(),
            started_at: storage::now(),
            completed_at: None,
            steps: [
                "transform",
                "upload assets",
                "upload cover",
                "create/update remote",
                "save mapping",
                "complete",
            ]
            .into_iter()
            .map(|name| Step {
                name: name.into(),
                status: "pending".into(),
                detail: String::new(),
            })
            .collect(),
            content: c,
            content_hash: String::new(),
            config_hash: storage::hash(public_config(p.as_ref(), cfg).to_string().as_bytes()),
            prepared: None,
            receipt: None,
            error: None,
            asset_directory: base.map(|p| p.to_string_lossy().into_owned()),
            assets_reused: 0,
            assets_uploaded: 0,
            asset_inputs: vec![],
            attempts: 1,
        };
        self.store.save_job(&job)?;
        self.run(job, cfg).await
    }
    pub async fn retry(&self, id: &str, cfg: &Config) -> Result<PublishJob> {
        let _guard = self.gate.lock().await;
        let mut job = self.store.job(id)?;
        if job.status == "success" {
            return Ok(job);
        }
        if job.status == "cancelled" {
            return Err(Error::new("validation", "已取消的任务不能重试"));
        }
        if job.status == "running" {
            return Err(Error::new("busy", "任务仍在执行"));
        }
        let p = self.registry.get(&job.extension_id)?;
        if job.config_hash != storage::hash(public_config(p.as_ref(), cfg).to_string().as_bytes()) {
            return Err(Error::new(
                "configuration",
                "任务创建后非密钥配置发生变化，请恢复原配置；凭据可以更正后重试",
            ));
        }
        job.status = "running".into();
        job.error = None;
        job.attempts += 1;
        self.store.save_job(&job)?;
        self.run(job, cfg).await
    }
    pub fn reconcile(&self, id: &str, receipt: Receipt) -> Result<()> {
        let mut job = self.store.job(id)?;
        if !matches!(job.status.as_str(), "needs_reconciliation" | "interrupted")
            || job.steps[3].status == "pending"
        {
            return Err(Error::new(
                "validation",
                "只有远端请求结果不确定的任务需要核对",
            ));
        }
        if receipt.id.is_empty() {
            return Err(Error::new("validation", "请提供已核对的远端 ID"));
        }
        job.receipt = Some(receipt);
        job.steps[3].status = "success".into();
        job.status = "failed".into();
        job.error = None;
        self.store.save_job(&job)
    }
    pub fn cancel(&self, id: &str) -> Result<()> {
        let mut j = self.store.job(id)?;
        if j.status == "running"
            || j.status == "success"
            || matches!(
                j.steps[3].status.as_str(),
                "running" | "uncertain" | "success"
            )
        {
            return Err(Error::new(
                "validation",
                "运行中、远端已成功或回执不确定的任务不能取消；请先核对并恢复映射",
            ));
        }
        j.status = "cancelled".into();
        j.completed_at = Some(storage::now());
        self.store.save_job(&j)
    }
    fn checkpoint(
        &self,
        j: &mut PublishJob,
        index: usize,
        status: &str,
        detail: &str,
    ) -> Result<()> {
        j.steps[index].status = status.into();
        j.steps[index].detail = detail.into();
        self.store.save_job(j)
    }
    async fn run(&self, mut j: PublishJob, cfg: &Config) -> Result<PublishJob> {
        let result = self.run_steps(&mut j, cfg).await;
        if let Err(error) = result {
            j.status = if error.code == "needs_reconciliation" {
                "needs_reconciliation"
            } else {
                "failed"
            }
            .into();
            j.error = Some(error.clone());
            for step in &mut j.steps {
                if step.status == "running" {
                    step.status = if j.status == "needs_reconciliation" {
                        "uncertain"
                    } else {
                        "failed"
                    }
                    .into();
                    step.detail = error.message.clone();
                }
            }
            self.store.save_job(&j)?;
        }
        Ok(j)
    }
    async fn run_steps(&self, j: &mut PublishJob, cfg: &Config) -> Result<()> {
        let publisher = self.registry.get(&j.extension_id)?;
        let mapping = self
            .store
            .mapping(&j.article_id, &j.target_id)?
            .filter(|m| m.status != "deleted");
        if j.steps[0].status != "success" {
            self.checkpoint(j, 0, "running", "")?;
            j.asset_inputs.clear();
            let mut input = j.content.clone();
            input
                .metadata
                .insert("_workbench_intent".into(), json!(j.action));
            input.metadata.insert(
                "_workbench_remote_id".into(),
                json!(mapping.as_ref().map(|m| &m.remote_id)),
            );
            let mut p = publisher.transform(&input, cfg).await?;
            p.job_id = j.job_id.clone();
            p.remote_id = mapping.as_ref().map(|m| m.remote_id.clone());
            p.content
                .metadata
                .insert("_workbench_intent".into(), json!(j.action));
            p.content.metadata.insert(
                "_workbench_revision".into(),
                json!(mapping.as_ref().and_then(|m| m.remote_revision.as_ref())),
            );
            if publisher.capabilities().contains(&"assets".into()) && j.action != Action::Delete {
                let mut inputs: Vec<(String, bool)> = transform::image_sources(&p.content.body)
                    .into_iter()
                    .map(|s| (s, false))
                    .collect();
                if let Some(c) = p.content.cover.as_ref().filter(|s| !s.is_empty()) {
                    inputs.push((c.clone(), true));
                }
                for (source, cover) in inputs {
                    let resolved = publisher.resolve_asset_source(&source, &p, j.action, cfg)?;
                    let asset = transform::load_asset(
                        &crate::core::client(),
                        &resolved,
                        j.asset_directory.as_deref().map(Path::new),
                    )
                    .await?;
                    let hash = storage::hash(&asset.bytes);
                    let extension = Path::new(&asset.name)
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("png");
                    std::fs::create_dir_all(&self.store.cache_dir)
                        .map_err(|_| Error::new("storage", "无法建立素材缓存目录"))?;
                    let path = self.store.cache_dir.join(format!("{hash}.{extension}"));
                    std::fs::write(&path, &asset.bytes)
                        .map_err(|_| Error::new("storage", "无法保存素材快照"))?;
                    j.asset_inputs.push(AssetInput {
                        source,
                        hash,
                        variant: publisher.asset_variant(cover, &p, j.action, cfg),
                        cover,
                        mime: asset.mime,
                        name: asset.name,
                        path: path.to_string_lossy().into_owned(),
                    });
                }
            }
            let mut public_cfg = cfg.clone();
            for (key, s) in publisher.manifest().schema["properties"]
                .as_object()
                .into_iter()
                .flatten()
            {
                if s["secret"] == true {
                    public_cfg.remove(key);
                }
            }
            j.content_hash=storage::hash(json!({"content":j.content,"action":j.action,"assets":j.asset_inputs.iter().map(|a|(&a.source,&a.hash,&a.variant)).collect::<Vec<_>>(),"config":public_cfg}).to_string().as_bytes());
            j.prepared = Some(p);
            self.checkpoint(j, 0, "success", "内容和素材已快照；重试使用此版本")?;
        }
        if let Some(m) = &mapping {
            if m.last_published_hash == j.content_hash
                && j.action != Action::Delete
                && m.status != "deleted"
            {
                j.operation = "unchanged".into();
                j.receipt = Some(Receipt {
                    id: m.remote_id.clone(),
                    url: None,
                    status: m.status.clone(),
                    revision: m.remote_revision.clone(),
                });
                for i in 1..4 {
                    self.checkpoint(j, i, "success", "内容未改变，跳过远端写入")?;
                }
            }
        }
        for (index, cover) in [(1, false), (2, true)] {
            if j.steps[index].status == "success" {
                continue;
            }
            self.checkpoint(j, index, "running", "")?;
            for input in j
                .asset_inputs
                .clone()
                .into_iter()
                .filter(|a| a.cover == cover)
            {
                let cached = self
                    .store
                    .asset(&input.hash, &j.target_id, &input.variant)?;
                let remote = if let Some((status, a)) = cached.filter(|(s, _)| s != "failed") {
                    if status != "available" {
                        return Err(Error::new(
                            "needs_reconciliation",
                            "素材请求可能已完成但回执不确定；请核对孤立素材，不会重复上传",
                        ));
                    }
                    j.assets_reused += 1;
                    a
                } else {
                    let bytes = std::fs::read(&input.path)
                        .map_err(|_| Error::new("asset", "重试所需素材快照丢失"))?;
                    if storage::hash(&bytes) != input.hash {
                        return Err(Error::new("asset", "素材快照哈希不匹配"));
                    }
                    let empty = RemoteAsset {
                        id: String::new(),
                        url: String::new(),
                    };
                    self.store.save_asset(
                        &input.hash,
                        &j.target_id,
                        &input.variant,
                        &j.job_id,
                        "pending",
                        &empty,
                    )?;
                    match publisher
                        .upload_asset(
                            transform::Asset {
                                bytes,
                                name: input.name.clone(),
                                mime: input.mime.clone(),
                            },
                            &input.variant,
                            cfg,
                        )
                        .await
                    {
                        Ok(a) => {
                            self.store.save_asset(
                                &input.hash,
                                &j.target_id,
                                &input.variant,
                                &j.job_id,
                                "available",
                                &a,
                            )?;
                            j.assets_uploaded += 1;
                            a
                        }
                        Err(e) => {
                            let uncertain =
                                matches!(e.code.as_str(), "network" | "api" | "storage");
                            self.store.save_asset(
                                &input.hash,
                                &j.target_id,
                                &input.variant,
                                &j.job_id,
                                if uncertain { "uncertain" } else { "failed" },
                                &empty,
                            )?;
                            return Err(if uncertain {
                                Error::new(
                                    "needs_reconciliation",
                                    "素材上传回执不确定，已停止；请核对远端素材",
                                )
                            } else {
                                e
                            });
                        }
                    }
                };
                let p = j
                    .prepared
                    .as_mut()
                    .ok_or_else(|| Error::new("storage", "缺少转换快照"))?;
                if !p
                    .asset_records
                    .iter()
                    .any(|a| a.id == remote.id && a.url == remote.url)
                {
                    p.asset_records.push(remote.clone());
                }
                if cover {
                    p.cover_asset = Some(remote.clone());
                    p.content.cover = Some(remote.url.clone());
                } else {
                    transform::replace_image(p, &input.source, &remote.url);
                    transform::replace_markdown_image(p, &input.source, &remote.url);
                }
                self.store.save_job(j)?;
            }
            self.checkpoint(
                j,
                index,
                "success",
                &format!("复用 {}，上传 {}", j.assets_reused, j.assets_uploaded),
            )?;
        }
        if j.steps[3].status != "success" {
            let mut p = j
                .prepared
                .clone()
                .ok_or_else(|| Error::new("storage", "缺少转换快照"))?;
            p.assets_processed = true;
            let remote_action = if j.action == Action::Delete {
                Action::Delete
            } else if mapping.is_some() && mapping.as_ref().is_some_and(|m| m.status != "deleted") {
                Action::Update
            } else {
                j.action
            };
            if remote_action == Action::Update
                && (!publisher.capabilities().contains(&"update".into())
                    || p.remote_id.as_deref().is_none_or(str::is_empty))
            {
                return Err(Error::new(
                    "unsupported",
                    "目标已有映射，但不支持更新或没有远端 ID；不会重复创建",
                ));
            }
            if matches!(j.steps[3].status.as_str(), "running" | "uncertain") {
                if let Some(receipt) = publisher.recover(&p, remote_action, cfg).await? {
                    j.receipt = Some(receipt);
                    self.checkpoint(j, 3, "success", "已核对远端回执")?;
                } else if !publisher.safe_remote_retry(cfg) {
                    return Err(Error::new(
                        "needs_reconciliation",
                        "远端请求结果不确定，请核对并填写 remote ID；不会盲目重新创建",
                    ));
                }
            }
            if j.steps[3].status != "success" {
                self.checkpoint(j, 3, "running", "正在提交远端")?;
                match publisher
                    .execute(
                        remote_action,
                        p,
                        cfg,
                        j.asset_directory.as_deref().map(Path::new),
                        mapping.as_ref().map(|m| m.remote_id.as_str()),
                    )
                    .await
                {
                    Ok(receipt) => {
                        j.receipt = Some(receipt);
                        self.checkpoint(j, 3, "success", "远端回执已持久化")?;
                    }
                    Err(e) => {
                        if matches!(
                            e.code.as_str(),
                            "network" | "api" | "storage" | "git_pending"
                        ) {
                            return Err(Error::new("needs_reconciliation", e.message));
                        }
                        return Err(e);
                    }
                }
            }
        }
        if j.steps[4].status != "success" {
            self.checkpoint(j, 4, "running", "")?;
            let r = j
                .receipt
                .as_ref()
                .ok_or_else(|| Error::new("storage", "缺少远端回执"))?;
            self.store.save_mapping(&Mapping {
                article_id: j.article_id.clone(),
                target_id: j.target_id.clone(),
                remote_id: r.id.clone(),
                remote_revision: r.revision.clone(),
                last_published_hash: j.content_hash.clone(),
                status: r.status.clone(),
            })?;
            self.store.link_assets(&j.job_id)?;
            for asset in &j.asset_inputs {
                self.store
                    .link_asset(&asset.hash, &j.target_id, &asset.variant)?;
            }
            self.checkpoint(j, 4, "success", "映射已保存")?;
        }
        j.status = "success".into();
        j.completed_at = Some(storage::now());
        self.checkpoint(j, 5, "success", "完成")
    }
}
