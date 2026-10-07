use crate::{
    content::Content,
    core::{Error, Result},
    publishing::{Action, Receipt, RemoteAsset},
    transform::Prepared,
};
use fs2::FileExt;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::Mutex};

pub fn hash(bytes: &[u8]) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(bytes))
}
pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
fn db_error(_: rusqlite::Error) -> Error {
    Error::new(
        "storage",
        "SQLite 操作失败，请检查磁盘空间、文件权限或数据库锁",
    )
}
fn encode<T: Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|_| Error::new("storage", "状态序列化失败"))
}
fn decode<T: serde::de::DeserializeOwned>(v: &str) -> Result<T> {
    serde_json::from_str(v).map_err(|_| Error::new("storage", "本地状态格式无效"))
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mapping {
    pub article_id: String,
    pub target_id: String,
    pub remote_id: String,
    pub remote_revision: Option<String>,
    pub last_published_hash: String,
    pub status: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Step {
    pub name: String,
    pub status: String,
    pub detail: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublishJob {
    pub job_id: String,
    pub article_id: String,
    pub target_id: String,
    pub extension_id: String,
    pub operation: String,
    pub action: Action,
    pub status: String,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub steps: Vec<Step>,
    pub content: Content,
    pub content_hash: String,
    pub config_hash: String,
    pub prepared: Option<Prepared>,
    pub receipt: Option<Receipt>,
    pub error: Option<Error>,
    pub asset_directory: Option<String>,
    pub assets_reused: u32,
    pub assets_uploaded: u32,
    pub asset_inputs: Vec<AssetInput>,
    pub attempts: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AssetInput {
    pub source: String,
    pub hash: String,
    pub variant: String,
    pub cover: bool,
    pub mime: String,
    pub name: String,
    pub path: String,
}
pub struct Store {
    db: Mutex<Connection>,
    pub cache_dir: std::path::PathBuf,
    _lock: Option<std::fs::File>,
}
impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path.with_extension("lock"))
            .map_err(|_| Error::new("storage", "无法打开数据库锁"))?;
        lock.try_lock_exclusive()
            .map_err(|_| Error::new("storage", "工作台数据库正在被另一进程使用"))?;
        let db = Connection::open(path).map_err(db_error)?;
        let mut store = Self::from_connection(db)?;
        store.cache_dir = path.with_extension("asset-cache");
        store._lock = Some(lock);
        Ok(store)
    }
    pub fn memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory().map_err(db_error)?)
    }
    fn from_connection(mut db: Connection) -> Result<Self> {
        db.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(db_error)?;
        db.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;",
        )
        .map_err(db_error)?;
        let version: u32 = db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(db_error)?;
        if version > 1 {
            return Err(Error::new(
                "migration",
                "数据库来自更新版本，请升级工作台，未修改数据库",
            ));
        }
        if version == 0 {
            let tx = db.transaction().map_err(db_error)?;
            tx.execute_batch(include_str!("001.sql"))
                .map_err(db_error)?;
            tx.pragma_update(None, "user_version", 1)
                .map_err(db_error)?;
            tx.commit().map_err(db_error)?;
        }
        // Process restart cannot assume that a remote request failed.
        db.execute(
            "UPDATE publish_jobs SET status='interrupted' WHERE status='running'",
            [],
        )
        .map_err(db_error)?;
        Ok(Self {
            db: Mutex::new(db),
            cache_dir: std::env::temp_dir()
                .join(format!("workbench-cache-{}", uuid::Uuid::new_v4())),
            _lock: None,
        })
    }
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.db
            .lock()
            .map_err(|_| Error::new("storage", "数据库状态不可用"))
    }
    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        self.lock()?
            .query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
                r.get(0)
            })
            .optional()
            .map_err(db_error)
    }
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.lock()?.execute("INSERT INTO settings VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,value]).map_err(db_error)?;
        Ok(())
    }
    pub fn source_article_id(&self, path: &str) -> Result<Option<String>> {
        self.lock()?.query_row("SELECT article_id FROM articles WHERE source_path=?1 ORDER BY updated_at DESC LIMIT 1",[path],|r|r.get(0)).optional().map_err(db_error)
    }
    pub fn save_article(&self, c: &Content) -> Result<()> {
        self.lock()?.execute("INSERT INTO articles VALUES (?1,?2,?3,?4) ON CONFLICT(article_id) DO UPDATE SET content_json=excluded.content_json,source_path=excluded.source_path,updated_at=excluded.updated_at",params![c.article_id,encode(c)?,c.source_path,now()]).map_err(db_error)?;
        Ok(())
    }
    pub fn save_target(
        &self,
        id: &str,
        extension: &str,
        public_config: &serde_json::Value,
    ) -> Result<()> {
        self.lock()?.execute("INSERT INTO targets VALUES (?1,?2,?3) ON CONFLICT(target_id) DO UPDATE SET config_json=excluded.config_json",params![id,extension,encode(public_config)?]).map_err(db_error)?;
        Ok(())
    }
    pub fn mapping(&self, article: &str, target: &str) -> Result<Option<Mapping>> {
        self.lock()?.query_row("SELECT article_id,target_id,remote_id,remote_revision,last_published_hash,status FROM remote_mappings WHERE article_id=?1 AND target_id=?2",params![article,target],|r|Ok(Mapping { article_id:r.get(0)?,target_id:r.get(1)?,remote_id:r.get(2)?,remote_revision:r.get(3)?,last_published_hash:r.get(4)?,status:r.get(5)? })).optional().map_err(db_error)
    }
    pub fn save_mapping(&self, m: &Mapping) -> Result<()> {
        self.lock()?.execute("INSERT INTO remote_mappings VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(article_id,target_id) DO UPDATE SET remote_id=excluded.remote_id,remote_revision=excluded.remote_revision,last_published_hash=excluded.last_published_hash,status=excluded.status",params![m.article_id,m.target_id,m.remote_id,m.remote_revision,m.last_published_hash,m.status]).map_err(db_error)?;
        Ok(())
    }
    pub fn mappings(&self, article: &str) -> Result<Vec<Mapping>> {
        let db = self.lock()?;
        let mut s=db.prepare("SELECT article_id,target_id,remote_id,remote_revision,last_published_hash,status FROM remote_mappings WHERE article_id=?1").map_err(db_error)?;
        let rows = s
            .query_map([article], |r| {
                Ok(Mapping {
                    article_id: r.get(0)?,
                    target_id: r.get(1)?,
                    remote_id: r.get(2)?,
                    remote_revision: r.get(3)?,
                    last_published_hash: r.get(4)?,
                    status: r.get(5)?,
                })
            })
            .map_err(db_error)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_error);
        rows
    }
    pub fn save_job(&self, job: &PublishJob) -> Result<()> {
        let mut db = self.lock()?;
        let tx = db.transaction().map_err(db_error)?;
        tx.execute("INSERT INTO publish_jobs VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(job_id) DO UPDATE SET operation=excluded.operation,status=excluded.status,completed_at=excluded.completed_at,job_json=excluded.job_json",params![job.job_id,job.article_id,job.target_id,job.extension_id,job.operation,job.status,job.started_at,job.completed_at,encode(job)?]).map_err(db_error)?;
        for (i, step) in job.steps.iter().enumerate() {
            tx.execute("INSERT INTO publish_steps VALUES (?1,?2,?3,?4,?5) ON CONFLICT(job_id,sequence) DO UPDATE SET status=excluded.status,detail=excluded.detail",params![job.job_id,i,step.name,step.status,step.detail]).map_err(db_error)?;
        }
        tx.commit().map_err(db_error)
    }
    pub fn job(&self, id: &str) -> Result<PublishJob> {
        let db = self.lock()?;
        let (raw, status): (String, String) = db
            .query_row(
                "SELECT job_json,status FROM publish_jobs WHERE job_id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(db_error)?;
        let mut job: PublishJob = decode(&raw)?;
        job.status = status;
        Ok(job)
    }
    pub fn jobs(&self) -> Result<Vec<PublishJob>> {
        let db = self.lock()?;
        let mut s=db.prepare("SELECT job_json,status FROM publish_jobs ORDER BY started_at DESC,rowid DESC LIMIT 200").map_err(db_error)?;
        let rows = s
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(db_error)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_error)?;
        rows.into_iter()
            .map(|(raw, status)| {
                let mut j: PublishJob = decode(&raw)?;
                j.status = status;
                Ok(j)
            })
            .collect()
    }
    pub fn pending(&self, article: &str, target: &str) -> Result<Option<PublishJob>> {
        let id:Option<String>=self.lock()?.query_row("SELECT job_id FROM publish_jobs WHERE article_id=?1 AND target_id=?2 AND status NOT IN ('success','cancelled') ORDER BY rowid DESC LIMIT 1",params![article,target],|r|r.get(0)).optional().map_err(db_error)?;
        id.map(|id| self.job(&id)).transpose()
    }
    pub fn asset(
        &self,
        hash: &str,
        target: &str,
        variant: &str,
    ) -> Result<Option<(String, RemoteAsset)>> {
        self.lock()?.query_row("SELECT status,remote_asset_id,remote_url FROM assets WHERE asset_hash=?1 AND target_id=?2 AND variant=?3",params![hash,target,variant],|r|Ok((r.get(0)?,RemoteAsset { id:r.get(1)?,url:r.get(2)? }))).optional().map_err(db_error)
    }
    pub fn save_asset(
        &self,
        hash: &str,
        target: &str,
        variant: &str,
        job: &str,
        status: &str,
        a: &RemoteAsset,
    ) -> Result<()> {
        self.lock()?.execute("INSERT INTO assets VALUES (?1,?2,?3,?4,?5,?6,?7,0) ON CONFLICT(asset_hash,target_id,variant) DO UPDATE SET remote_asset_id=excluded.remote_asset_id,remote_url=excluded.remote_url,status=excluded.status",params![hash,target,variant,a.id,a.url,status,job]).map_err(db_error)?;
        Ok(())
    }
    pub fn link_assets(&self, job: &str) -> Result<()> {
        self.lock()?
            .execute(
                "UPDATE assets SET linked=1 WHERE created_by_job=?1 AND status='available'",
                [job],
            )
            .map_err(db_error)?;
        Ok(())
    }
    pub fn link_asset(&self, hash: &str, target: &str, variant: &str) -> Result<()> {
        self.lock()?
            .execute(
                "UPDATE assets SET linked=1 WHERE asset_hash=?1 AND target_id=?2 AND variant=?3",
                params![hash, target, variant],
            )
            .map_err(db_error)?;
        Ok(())
    }
    pub fn orphans(&self) -> Result<serde_json::Value> {
        let db = self.lock()?;
        let mut s=db.prepare("SELECT asset_hash,target_id,variant,remote_asset_id,remote_url,status FROM assets WHERE linked=0").map_err(db_error)?;
        let rows=s.query_map([],|r|Ok(serde_json::json!({"asset_hash":r.get::<_,String>(0)?,"target_id":r.get::<_,String>(1)?,"variant":r.get::<_,String>(2)?,"remote_asset_id":r.get::<_,String>(3)?,"remote_url":r.get::<_,String>(4)?,"status":r.get::<_,String>(5)?}))).map_err(db_error)?.collect::<std::result::Result<Vec<_>,_>>().map_err(db_error)?;
        Ok(serde_json::json!(rows))
    }
}
