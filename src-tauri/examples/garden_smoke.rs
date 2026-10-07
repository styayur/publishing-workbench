use publishing_workbench::{
    content::Content,
    core::Config,
    publishing::{reliable::ReliablePublishing, Action},
    storage::Store,
};
use serde_json::json;
use std::{path::PathBuf, sync::Arc};
#[tokio::main]
async fn main() {
    let root = PathBuf::from(std::env::args().nth(1).expect("content repository path"));
    let data = PathBuf::from(std::env::args().nth(2).expect("test state directory"));
    std::fs::create_dir_all(&data).unwrap();
    let store = Arc::new(Store::open(data.join("garden-smoke.db")).unwrap());
    let core = ReliablePublishing::new(store.clone());
    let existing = store
        .source_article_id("garden-safe-draft-fixture")
        .unwrap();
    let mut c=Content{title:"Publishing Workbench safe draft fixture".into(),slug:"workbench-v02-safe-draft".into(),summary:"Draft-only integration fixture, never intended for production publication.".into(),body:"# Safe draft\n\nWORKBENCH_V02_DRAFT_BOUNDARY_CANARY\n\nThis draft validates the existing Digital Garden publication gate.".into(),format:"mdx".into(),source_path:Some("garden-safe-draft-fixture".into()),..Default::default()};
    if let Some(id) = existing {
        c.article_id = id;
    }
    let cfg:Config=json!({"repository_path":root.to_string_lossy(),"profile":"Digital Garden Engine","auto_commit":"false","auto_push":"false"}).as_object().unwrap().clone();
    let j = core
        .start("git-content", Action::CreateDraft, c.clone(), &cfg, None)
        .await
        .unwrap();
    assert_eq!(j.status, "success", "{:?}", j.error);
    c.body
        .push_str("\n\nUpdated through the same article mapping.");
    let j = core
        .start("git-content", Action::CreateDraft, c, &cfg, None)
        .await
        .unwrap();
    assert_eq!(j.status, "success", "{:?}", j.error);
    assert_eq!(j.operation, "update");
    println!("{}",serde_json::to_string_pretty(&json!({"article_id":j.article_id,"operation":j.operation,"receipt":j.receipt,"steps":j.steps})).unwrap());
}
