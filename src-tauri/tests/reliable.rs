use publishing_workbench::{
    content::Content,
    core::Config,
    extensions::{
        git_content::{checked_path, document},
        wechat::WeChat,
    },
    publishing::{reliable::ReliablePublishing, Action, Registry},
    storage::{hash, Store},
};
use serde_json::{json, Value};
use std::{fs, path::Path, process::Command, sync::Arc};
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
};
fn c() -> Content {
    Content {
        title: "Reliable test".into(),
        slug: "reliable-test".into(),
        summary: "A safe fixture".into(),
        body: "# Test\n\nBody".into(),
        ..Default::default()
    }
}
fn cfg(v: Value) -> Config {
    v.as_object().unwrap().clone()
}
fn core() -> ReliablePublishing {
    ReliablePublishing::new(Arc::new(Store::memory().unwrap()))
}
fn git(root: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap()
}
fn repo() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    git(t.path(), &["init"]);
    git(t.path(), &["config", "user.email", "test@example.invalid"]);
    git(t.path(), &["config", "user.name", "Workbench test"]);
    fs::write(t.path().join(".keep"), "baseline").unwrap();
    git(t.path(), &["add", ".keep"]);
    git(t.path(), &["commit", "-m", "baseline"]);
    t
}
fn gitcfg(t: &Path, commit: bool) -> Config {
    cfg(
        json!({"repository_path":t.to_string_lossy(),"profile":"Digital Garden Engine","auto_commit":if commit{"true"}else{"false"},"auto_push":"false"}),
    )
}
#[test]
fn stable_article_id_and_sha256() {
    let a = c();
    let mut b: Content = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
    b.title = "Changed".into();
    b.ensure_id();
    assert_eq!(a.article_id, b.article_id);
    assert_ne!(c().article_id, a.article_id);
    assert_eq!(
        hash(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}
#[test]
fn sqlite_migration_lock_and_source_identity() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("state.db");
    let s = Store::open(&p).unwrap();
    assert!(Store::open(&p).is_err());
    let mut a = c();
    a.source_path = Some("D:/articles/a.md".into());
    s.save_article(&a).unwrap();
    assert_eq!(
        s.source_article_id("D:/articles/a.md").unwrap(),
        Some(a.article_id)
    );
    drop(s);
    let s = Store::open(&p).unwrap();
    assert!(s.jobs().unwrap().is_empty());
    drop(s);
    let db = rusqlite::Connection::open(&p).unwrap();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i32>(0))
            .unwrap(),
        1
    );
    for table in [
        "articles",
        "targets",
        "remote_mappings",
        "assets",
        "publish_jobs",
        "publish_steps",
    ] {
        db.prepare(&format!("SELECT * FROM {table}")).unwrap();
    }
    db.pragma_update(None, "user_version", 99).unwrap();
    drop(db);
    assert_eq!(Store::open(p).err().unwrap().code, "migration");
}
#[tokio::test]
async fn generic_create_update_unchanged_and_mapping() {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/articles"))
        .and(wiremock::matchers::header_exists("X-Article-Id"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id":"42"})))
        .expect(1)
        .mount(&s)
        .await;
    Mock::given(method("PUT"))
        .and(path("/articles/42"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("ETag", "rev2")
                .set_body_json(json!({"id":"42"})),
        )
        .expect(1)
        .mount(&s)
        .await;
    let cfg = cfg(
        json!({"endpoint":format!("{}/articles",s.uri()),"method":"POST","update_endpoint":format!("{}/articles/{{{{remote_id}}}}",s.uri()),"update_method":"PUT","template":"{\"id\":\"{{article_id}}\",\"remote\":\"{{remote_id}}\"}"}),
    );
    let p = core();
    let mut a = c(); // header match uses any UUID below
    let first = p
        .start("generic-rest", Action::CreateDraft, a.clone(), &cfg, None)
        .await
        .unwrap();
    assert_eq!(first.status, "success");
    a.body.push_str(" updated");
    let next = p
        .start("generic-rest", Action::CreateDraft, a.clone(), &cfg, None)
        .await
        .unwrap();
    assert_eq!(next.operation, "update");
    assert_eq!(next.status, "success");
    let unchanged = p
        .start("generic-rest", Action::CreateDraft, a, &cfg, None)
        .await
        .unwrap();
    assert_eq!(unchanged.operation, "unchanged");
    let m = p
        .store
        .mapping(&first.article_id, &first.target_id)
        .unwrap()
        .unwrap();
    assert_eq!(m.remote_id, "42");
    assert_eq!(m.remote_revision.as_deref(), Some("rev2"));
}

#[tokio::test]
async fn retry_known_failure_reuses_job_and_credentials_can_change() {
    let s = MockServer::start().await;
    let cfg = cfg(json!({"endpoint":s.uri(),"template":"{}","token":"wrong"}));
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&s)
        .await;
    let p = core();
    let j = p
        .start("generic-rest", Action::CreateDraft, c(), &cfg, None)
        .await
        .unwrap();
    assert_eq!(j.status, "failed");
    s.reset().await;
    Mock::given(method("POST"))
        .and(header("Idempotency-Key", j.job_id.as_str()))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id":"retry-1"})))
        .expect(1)
        .mount(&s)
        .await;
    let mut fixed = cfg;
    fixed.insert("token".into(), json!("corrected"));
    let result = p.retry(&j.job_id, &fixed).await.unwrap();
    assert_eq!(result.status, "success");
    assert_eq!(result.attempts, 2);
    assert_eq!(p.retry(&j.job_id, &fixed).await.unwrap().status, "success");
}
#[tokio::test]
async fn uncertain_create_does_not_repeat_and_manual_reconciliation() {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&s)
        .await;
    let cfg = cfg(json!({"endpoint":s.uri(),"template":"{}"}));
    let p = core();
    let a = c();
    let j = p
        .start("generic-rest", Action::CreateDraft, a.clone(), &cfg, None)
        .await
        .unwrap();
    assert_eq!(j.status, "needs_reconciliation");
    assert_eq!(
        p.retry(&j.job_id, &cfg).await.unwrap().status,
        "needs_reconciliation"
    );
    assert!(p.cancel(&j.job_id).is_err());
    assert_eq!(
        p.start("generic-rest", Action::CreateDraft, a, &cfg, None)
            .await
            .err()
            .unwrap()
            .code,
        "pending_job"
    );
    p.reconcile(
        &j.job_id,
        publishing_workbench::publishing::Receipt {
            id: "verified".into(),
            url: None,
            status: "draft".into(),
            revision: None,
        },
    )
    .unwrap();
    assert_eq!(p.retry(&j.job_id, &cfg).await.unwrap().status, "success");
}
#[tokio::test]
async fn interrupted_after_remote_receipt_resumes_without_request() {
    let t = tempfile::tempdir().unwrap();
    let db = t.path().join("state.db");
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id":"once"})))
        .expect(1)
        .mount(&s)
        .await;
    let cfg = cfg(json!({"endpoint":s.uri(),"template":"{}"}));
    let p = ReliablePublishing::new(Arc::new(Store::open(&db).unwrap()));
    let mut j = p
        .start("generic-rest", Action::CreateDraft, c(), &cfg, None)
        .await
        .unwrap();
    j.status = "running".into();
    j.steps[4].status = "pending".into();
    j.steps[5].status = "pending".into();
    p.store.save_job(&j).unwrap();
    drop(p);
    let p = ReliablePublishing::new(Arc::new(Store::open(db).unwrap()));
    assert_eq!(p.store.job(&j.job_id).unwrap().status, "interrupted");
    assert_eq!(p.retry(&j.job_id, &cfg).await.unwrap().status, "success");
}
#[tokio::test]
async fn git_dirty_rejected_and_create_update_preserves_boundary() {
    let t = repo();
    let cfg = gitcfg(t.path(), false);
    let p = core();
    fs::write(t.path().join("user.txt"), "user edit").unwrap();
    let mut a = c();
    let j = p
        .start("git-content", Action::CreateDraft, a.clone(), &cfg, None)
        .await
        .unwrap();
    assert_eq!(j.status, "failed");
    assert_eq!(j.error.unwrap().code, "git_dirty");
    assert!(!t
        .path()
        .join("content/drafts/posts/reliable-test.md")
        .exists());
    p.cancel(&j.job_id).unwrap();
    fs::remove_file(t.path().join("user.txt")).unwrap();
    let j = p
        .start("git-content", Action::CreateDraft, a.clone(), &cfg, None)
        .await
        .unwrap();
    assert_eq!(j.status, "success", "{:?}", j.error);
    a.body.push_str(" updated");
    let j = p
        .start("git-content", Action::CreateDraft, a.clone(), &cfg, None)
        .await
        .unwrap();
    assert_eq!(j.operation, "update");
    assert_eq!(j.status, "success", "{:?}", j.error);
    let file = t.path().join("content/drafts/posts/reliable-test.md");
    assert!(fs::read_to_string(&file)
        .unwrap()
        .contains("status: \"draft\""));
    assert!(!t.path().join("content/published").exists());
    fs::write(file, "user edit").unwrap();
    a.body.push_str(" changed again");
    let j = p
        .start("git-content", Action::CreateDraft, a, &cfg, None)
        .await
        .unwrap();
    assert_eq!(j.status, "failed");
    assert_eq!(j.error.unwrap().code, "git_dirty");
}
#[test]
fn digital_garden_profiles_and_path_boundary() {
    let a = c();
    for k in ["Writing", "Project", "Library"] {
        let cfg = cfg(json!({"content_kind":k}));
        let text = document(&a, &cfg, "draft").unwrap();
        assert!(text.contains("status: \"draft\""));
        assert!(!text.contains("article_id:"));
        if k == "Writing" {
            assert!(text.contains("maturity:"));
        }
        if k == "Project" {
            assert!(text.contains("projectStatus:"));
        }
        if k == "Library" {
            assert!(text.contains("kind:"));
        }
    }
    let t = repo();
    for bad in ["../outside", ".git/config", "/absolute"] {
        assert!(checked_path(t.path(), bad).is_err());
    }
}
#[tokio::test]
async fn git_commit_mdx_promote_update_delete() {
    let t = repo();
    let cfg = gitcfg(t.path(), true);
    let p = core();
    let mut a = c();
    a.format = "mdx".into();
    let j = p
        .start("git-content", Action::CreateDraft, a.clone(), &cfg, None)
        .await
        .unwrap();
    assert_eq!(j.status, "success", "{:?}", j.error);
    assert!(git(t.path(), &["status", "--porcelain"]).is_empty());
    let j = p
        .start("git-content", Action::Publish, a.clone(), &cfg, None)
        .await
        .unwrap();
    assert_eq!(j.status, "success", "{:?}", j.error);
    assert!(t
        .path()
        .join("content/published/posts/reliable-test.mdx")
        .exists());
    assert!(!t
        .path()
        .join("content/drafts/posts/reliable-test.mdx")
        .exists());
    a.body.push_str(" edit");
    let j = p
        .start("git-content", Action::Update, a.clone(), &cfg, None)
        .await
        .unwrap();
    assert_eq!(j.status, "success", "{:?}", j.error);
    let j = p
        .start("git-content", Action::Delete, a, &cfg, None)
        .await
        .unwrap();
    assert_eq!(j.status, "success", "{:?}", j.error);
    assert!(!t
        .path()
        .join("content/published/posts/reliable-test.mdx")
        .exists());
}
#[tokio::test]
async fn draft_cannot_use_published_path_even_generic_profile() {
    let t = repo();
    let mut cfg = gitcfg(t.path(), false);
    cfg.insert("draft_path".into(), json!("content/published"));
    let p = core();
    let j = p
        .start("git-content", Action::CreateDraft, c(), &cfg, None)
        .await
        .unwrap();
    assert_eq!(j.error.unwrap().code, "boundary");
    assert!(!t.path().join("content/published").exists());
}
#[tokio::test]
async fn wordpress_assets_dedup_retry_and_update_regression() {
    let s = MockServer::start().await;
    let t = tempfile::tempdir().unwrap();
    fs::write(t.path().join("a.png"), b"\x89PNG\r\n\x1a\nfixture").unwrap();
    let cfg = cfg(json!({"url":s.uri(),"username":"writer","password":"secret"}));
    Mock::given(method("POST"))
        .and(path("/wp-json/wp/v2/media"))
        .respond_with(
            ResponseTemplate::new(201)
                .set_body_json(json!({"id":8,"source_url":"https://cdn.example/a.png"})),
        )
        .expect(1)
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/wp-json/wp/v2/posts"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&s)
        .await;
    let p = core();
    let mut a = c();
    a.body = "![same](a.png)\n\n![same again](a.png)".into();
    a.cover = Some("a.png".into());
    let j = p
        .start(
            "wordpress",
            Action::CreateDraft,
            a.clone(),
            &cfg,
            Some(t.path()),
        )
        .await
        .unwrap();
    assert_eq!(j.status, "failed");
    assert_eq!(j.assets_uploaded, 1);
    assert!(j.assets_reused >= 1);
    assert_eq!(p.store.orphans().unwrap().as_array().unwrap().len(), 1);
    s.reset().await;
    Mock::given(method("POST"))
        .and(path("/wp-json/wp/v2/posts"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id":77,"status":"draft"})))
        .expect(1)
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/wp-json/wp/v2/posts/77"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"id":77,"status":"draft","modified_gmt":"revision2"})),
        )
        .expect(1)
        .mount(&s)
        .await;
    assert_eq!(p.retry(&j.job_id, &cfg).await.unwrap().status, "success");
    a.body.push_str(" updated");
    let j = p
        .start("wordpress", Action::CreateDraft, a, &cfg, Some(t.path()))
        .await
        .unwrap();
    assert_eq!(j.status, "success", "{:?}", j.error);
    assert_eq!(j.assets_uploaded, 0);
    assert_eq!(j.operation, "update");
    assert!(p.store.orphans().unwrap().as_array().unwrap().is_empty());
}
#[tokio::test]
async fn wechat_registry_assets_draft_update_regression() {
    let s = MockServer::start().await;
    let t = tempfile::tempdir().unwrap();
    fs::write(t.path().join("a.png"), b"\x89PNG\r\n\x1a\nfixture").unwrap();
    Mock::given(method("POST"))
        .and(path("/cgi-bin/stable_token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"access_token":"mock","expires_in":7200})),
        )
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/cgi-bin/media/uploadimg"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"url":"https://mmbiz.example/a.png"})),
        )
        .expect(1)
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/cgi-bin/material/add_material"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"media_id":"cover"})))
        .expect(1)
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/cgi-bin/draft/add"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"media_id":"draft"})))
        .expect(1)
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/cgi-bin/draft/update"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"errcode":0})))
        .expect(1)
        .mount(&s)
        .await;
    let p = ReliablePublishing::with_registry(
        Arc::new(Store::memory().unwrap()),
        Registry::new(vec![Arc::new(WeChat::with_base(&s.uri()))]),
    );
    let cfg = cfg(json!({"appid":"app","secret":"secret"}));
    let mut a = c();
    a.cover = Some("a.png".into());
    a.body = "![body](a.png)".into();
    let j = p
        .start(
            "wechat",
            Action::CreateDraft,
            a.clone(),
            &cfg,
            Some(t.path()),
        )
        .await
        .unwrap();
    assert_eq!(j.status, "success", "{:?}", j.error);
    a.body.push_str(" updated");
    let j = p
        .start(
            "wechat",
            Action::CreateDraft,
            a.clone(),
            &cfg,
            Some(t.path()),
        )
        .await
        .unwrap();
    assert_eq!(j.status, "success", "{:?}", j.error);
    assert_eq!(j.operation, "update");
    assert_eq!(j.assets_uploaded, 0);
    assert_eq!(
        p.start("wechat", Action::Publish, a, &cfg, None)
            .await
            .err()
            .unwrap()
            .code,
        "unsupported"
    );
}

#[tokio::test]
async fn git_push_failure_resumes_known_transaction_without_duplicate_commit() {
    let t = repo();
    let p = core();
    let mut cfg = gitcfg(t.path(), true);
    cfg.insert("auto_push".into(), json!("true"));
    let j = p
        .start("git-content", Action::CreateDraft, c(), &cfg, None)
        .await
        .unwrap();
    assert_eq!(j.status, "needs_reconciliation");
    let head = git(t.path(), &["rev-parse", "HEAD"]);
    let bare = tempfile::tempdir().unwrap();
    git(bare.path(), &["init", "--bare"]);
    git(
        t.path(),
        &["remote", "add", "origin", bare.path().to_str().unwrap()],
    );
    let branch = git(t.path(), &["branch", "--show-current"]);
    git(
        t.path(),
        &[
            "config",
            &format!("branch.{}.remote", branch.trim()),
            "origin",
        ],
    );
    git(
        t.path(),
        &[
            "config",
            &format!("branch.{}.merge", branch.trim()),
            &format!("refs/heads/{}", branch.trim()),
        ],
    );
    let resumed = p.retry(&j.job_id, &cfg).await.unwrap();
    assert_eq!(resumed.status, "success", "{:?}", resumed.error);
    assert_eq!(git(t.path(), &["rev-parse", "HEAD"]), head);
    assert_eq!(git(bare.path(), &["rev-parse", branch.trim()]), head);
}
#[tokio::test]
async fn git_assets_are_deduped_and_scoped_on_promotion() {
    let t = repo();
    let image = tempfile::tempdir().unwrap();
    fs::write(image.path().join("a.png"), b"\x89PNG\r\n\x1a\nfixture").unwrap();
    let p = core();
    let cfg = gitcfg(t.path(), true);
    let mut a = c();
    a.body = "![same](a.png)\n\n![again](a.png)".into();
    a.cover = Some("a.png".into());
    let j = p
        .start(
            "git-content",
            Action::CreateDraft,
            a.clone(),
            &cfg,
            Some(image.path()),
        )
        .await
        .unwrap();
    assert_eq!(j.status, "success", "{:?}", j.error);
    assert_eq!(j.assets_uploaded, 1);
    assert!(!t.path().join("content/published/assets").exists());
    let text = fs::read_to_string(t.path().join("content/drafts/posts/reliable-test.md")).unwrap();
    assert!(text.contains("/garden-assets/"));
    assert!(!text.contains("](a.png)"));
    let j = p
        .start("git-content", Action::Publish, a, &cfg, Some(image.path()))
        .await
        .unwrap();
    assert_eq!(j.status, "success", "{:?}", j.error);
    assert_eq!(
        fs::read_dir(t.path().join("content/published/assets"))
            .unwrap()
            .count(),
        1
    );
    assert!(git(t.path(), &["status", "--porcelain"]).is_empty());
}
#[tokio::test]
async fn rest_idempotent_contract_retries_same_key() {
    let s = MockServer::start().await;
    let cfg = cfg(json!({"endpoint":s.uri(),"template":"{}","idempotency_supported":"true"}));
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&s)
        .await;
    let p = core();
    let j = p
        .start("generic-rest", Action::CreateDraft, c(), &cfg, None)
        .await
        .unwrap();
    s.reset().await;
    Mock::given(method("POST"))
        .and(header("Idempotency-Key", j.job_id.as_str()))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id":"idempotent"})))
        .expect(1)
        .mount(&s)
        .await;
    assert_eq!(p.retry(&j.job_id, &cfg).await.unwrap().status, "success");
}
