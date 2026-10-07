CREATE TABLE articles(article_id TEXT PRIMARY KEY,content_json TEXT NOT NULL,source_path TEXT,updated_at INTEGER NOT NULL);
CREATE TABLE targets(target_id TEXT PRIMARY KEY,extension_id TEXT NOT NULL,config_json TEXT NOT NULL);
CREATE TABLE remote_mappings(article_id TEXT NOT NULL,target_id TEXT NOT NULL,remote_id TEXT NOT NULL,remote_revision TEXT,last_published_hash TEXT NOT NULL,status TEXT NOT NULL,PRIMARY KEY(article_id,target_id));
CREATE TABLE assets(asset_hash TEXT NOT NULL,target_id TEXT NOT NULL,variant TEXT NOT NULL,remote_asset_id TEXT NOT NULL,remote_url TEXT NOT NULL,status TEXT NOT NULL,created_by_job TEXT NOT NULL,linked INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(asset_hash,target_id,variant));
CREATE TABLE publish_jobs(job_id TEXT PRIMARY KEY,article_id TEXT NOT NULL,target_id TEXT NOT NULL,extension_id TEXT NOT NULL,operation TEXT NOT NULL,status TEXT NOT NULL,started_at INTEGER NOT NULL,completed_at INTEGER,job_json TEXT NOT NULL);
CREATE TABLE publish_steps(job_id TEXT NOT NULL REFERENCES publish_jobs(job_id),sequence INTEGER NOT NULL,name TEXT NOT NULL,status TEXT NOT NULL,detail TEXT NOT NULL,PRIMARY KEY(job_id,sequence));
CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL);
CREATE UNIQUE INDEX one_active_job ON publish_jobs(article_id,target_id) WHERE status NOT IN ('success','cancelled');
