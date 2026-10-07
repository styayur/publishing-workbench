use publishing_workbench::{
    content::Content,
    core::{self, Config},
    extensions::{
        generic_rest::{payload, GenericRest},
        wechat::WeChat,
        wordpress::WordPress,
    },
    publishing::{Action, Publisher, Registry},
    transform,
};
use serde_json::{json, Value};
use wiremock::{
    matchers::{body_json, body_partial_json, header, method, path, query_param},
    Mock, MockServer, ResponseTemplate,
};
fn content() -> Content {
    Content {
        title: "Hello \"世界\"".into(),
        slug: "hello".into(),
        summary: "Summary".into(),
        body: "# Heading\n\n**Bold** & text".into(),
        authors: vec!["Writer".into()],
        tags: vec![],
        ..Default::default()
    }
}
fn config(v: Value) -> Config {
    v.as_object().unwrap().clone()
}

#[test]
fn canonical_roundtrip_and_validation() {
    let c = content();
    let encoded = serde_json::to_string(&c).unwrap();
    assert_eq!(serde_json::from_str::<Content>(&encoded).unwrap(), c);
    assert!(c.validate().is_ok());
    assert!(Content::default().validate().is_err());
}
#[test]
fn markdown_is_sanitized_and_images_detected() {
    let mut c = content();
    c.body="# Heading\n\n**Bold**\n\n<script>alert('x')</script>\n\n![alt](images/a.png?x=1&y=2)\n\n<img src=\"https://example.com/b.png\">\n\n`![not-image](fake.png)`".into();
    let mut p = transform::markdown(&c);
    assert!(p.html.contains("<h1>Heading</h1>"));
    assert!(p.html.contains("<strong>Bold</strong>"));
    assert!(!p.html.contains("<script"));
    assert_eq!(
        transform::image_sources(&c.body),
        vec!["images/a.png?x=1&y=2", "https://example.com/b.png"]
    );
    transform::replace_image(
        &mut p,
        "images/a.png?x=1&y=2",
        "https://cdn.example.com/new.png",
    );
    assert!(p.html.contains("src=\"https://cdn.example.com/new.png\""));
}
#[test]
fn registry_and_capability_detection() {
    let r = Registry::builtin();
    assert_eq!(r.manifests().len(), 4);
    assert!(r.get("missing").is_err());
    let wx = r.get("wechat").unwrap();
    assert!(wx.capabilities().contains(&"draft".into()));
    assert!(!wx.capabilities().contains(&"publish".into()));
    assert!(r
        .get("wordpress")
        .unwrap()
        .capabilities()
        .contains(&"update".into()));
}
#[test]
fn generic_payload_preserves_types_and_escapes() {
    let mut c = content();
    c.tags = vec!["a".into(), "b".into()];
    c.metadata.insert("custom".into(), json!(123));
    let p = transform::markdown(&c);
    let value=payload(r#"{"title":"{{title}}","tags":"{{tags}}","meta":"{{metadata}}","text":"Prefix {{summary}}","action":"{{action}}","nested":["{{cover}}"]}"#,&p,Action::CreateDraft).unwrap();
    assert_eq!(value["title"], c.title);
    assert_eq!(value["tags"], json!(["a", "b"]));
    assert_eq!(value["meta"]["custom"], 123);
    assert_eq!(value["nested"][0], Value::Null);
    assert_eq!(value["action"], "createDraft");
    assert_eq!(value["text"], "Prefix Summary");
    assert!(payload(r#"{"x":"{{unknown}}"}"#, &p, Action::Publish).is_err());
    assert!(payload("not json", &p, Action::Publish).is_err());
}
#[tokio::test]
async fn dispatch_rejects_unsupported_before_network() {
    let r = Registry::builtin();
    let error = r
        .dispatch(
            "wechat",
            Action::Publish,
            &content(),
            &Config::new(),
            None,
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "unsupported");
    let error = r
        .dispatch(
            "wordpress",
            Action::Update,
            &content(),
            &Config::new(),
            None,
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "validation");
}
#[tokio::test]
async fn wordpress_connection_draft_publish_update() {
    let server = MockServer::start().await;
    let cfg = config(
        json!({"url":server.uri(),"username":"editor","password":"test-password","categories":"1,2"}),
    );
    let wp = WordPress::new();
    Mock::given(method("GET"))
        .and(path("/wp-json/wp/v2/users/me"))
        .and(header(
            "authorization",
            "Basic ZWRpdG9yOnRlc3QtcGFzc3dvcmQ=",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":1})))
        .expect(1)
        .mount(&server)
        .await;
    wp.test_connection(&cfg).await.unwrap();
    for (status, action) in [("draft", Action::CreateDraft), ("publish", Action::Publish)] {
        Mock::given(method("POST"))
            .and(path("/wp-json/wp/v2/posts"))
            .and(body_partial_json(
                json!({"status":status,"categories":[1,2],"slug":"hello"}),
            ))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(
                    json!({"id":7,"status":status,"link":"https://example.com/hello"}),
                ),
            )
            .expect(1)
            .mount(&server)
            .await;
        let receipt = wp
            .execute(action, transform::markdown(&content()), &cfg, None, None)
            .await
            .unwrap();
        assert_eq!(receipt.status, status);
    }
    Mock::given(method("POST"))
        .and(path("/wp-json/wp/v2/posts/7"))
        .and(body_partial_json(json!({"title":content().title})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":7,"status":"publish"})))
        .expect(1)
        .mount(&server)
        .await;
    wp.update(transform::markdown(&content()), &cfg, None, "7")
        .await
        .unwrap();
    let requests = server.received_requests().await.unwrap();
    let updated: Value = serde_json::from_slice(&requests.last().unwrap().body).unwrap();
    assert!(updated.get("status").is_none());
}
#[tokio::test]
async fn wordpress_assets_tags_and_publisher_dispatch() {
    let server = MockServer::start().await;
    let cfg = config(json!({"url":server.uri(),"username":"editor","password":"secret"}));
    let mut c = content();
    c.body = format!("![pic]({}/image.png)", server.uri());
    c.cover = Some(format!("{}/image.png", server.uri()));
    c.tags = vec!["existing".into(), "new".into()];
    c.metadata.insert("author_id".into(), json!(3));
    Mock::given(method("GET"))
        .and(path("/image.png"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![1, 2, 3]))
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/wp-json/wp/v2/media"))
        .respond_with(
            ResponseTemplate::new(201)
                .set_body_json(json!({"id":11,"source_url":"https://cdn.example.com/image.png"})),
        )
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/wp-json/wp/v2/tags"))
        .and(query_param("search", "existing"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!([{"name":"existing","id":21}])),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/wp-json/wp/v2/tags"))
        .and(query_param("search", "new"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/wp-json/wp/v2/tags"))
        .and(body_json(json!({"name":"new"})))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id":22})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/wp-json/wp/v2/posts"))
        .and(body_partial_json(
            json!({"featured_media":11,"tags":[21,22],"author":3,"status":"draft"}),
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id":99,"status":"draft"})))
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(
        Registry::builtin()
            .dispatch("wordpress", Action::CreateDraft, &c, &cfg, None, None)
            .await
            .unwrap()
            .id,
        "99"
    );
    let requests = server.received_requests().await.unwrap();
    let posted: Value = serde_json::from_slice(&requests.last().unwrap().body).unwrap();
    assert!(posted["content"]
        .as_str()
        .unwrap()
        .contains("https://cdn.example.com/image.png"));
    assert!(!posted["content"].as_str().unwrap().contains(&server.uri()));
}
#[tokio::test]
async fn wechat_complete_draft_pipeline_and_token_cache() {
    let server = MockServer::start().await;
    let cfg = config(json!({"appid":"test-app","secret":"test-secret"}));
    let wx = WeChat::with_base(&server.uri());
    let mut c = content();
    c.body = format!("段落\n\n![pic]({}/image.png)", server.uri());
    c.cover = Some(format!("{}/image.png", server.uri()));
    Mock::given(method("POST")).and(path("/cgi-bin/stable_token")).and(body_json(json!({"grant_type":"client_credential","appid":"test-app","secret":"test-secret","force_refresh":false}))).respond_with(ResponseTemplate::new(200).set_body_json(json!({"access_token":"test-token","expires_in":7200}))).expect(1).mount(&server).await;
    wx.test_connection(&cfg).await.unwrap();
    Mock::given(method("GET"))
        .and(path("/image.png"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![1, 2, 3]))
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/cgi-bin/media/uploadimg"))
        .and(query_param("access_token", "test-token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"url":"https://mmbiz.qpic.cn/image.png"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/cgi-bin/material/add_material"))
        .and(query_param("type", "image"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"media_id":"cover-id"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/cgi-bin/draft/add"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"media_id":"draft-id"})))
        .expect(1)
        .mount(&server)
        .await;
    let p = wx.transform(&c, &cfg).await.unwrap();
    assert!(!p.warnings.is_empty());
    let receipt = wx.create_draft(p, &cfg, None).await.unwrap();
    assert_eq!(receipt.id, "draft-id");
    assert_eq!(receipt.status, "draft");
    let requests = server.received_requests().await.unwrap();
    let posted: Value = serde_json::from_slice(&requests.last().unwrap().body).unwrap();
    assert_eq!(posted["articles"][0]["thumb_media_id"], "cover-id");
    assert!(posted["articles"][0]["content"]
        .as_str()
        .unwrap()
        .contains("https://mmbiz.qpic.cn/image.png"));
    assert_eq!(
        wx.publish(transform::markdown(&c), &cfg, None)
            .await
            .unwrap_err()
            .code,
        "unsupported"
    );
}
#[tokio::test]
async fn wechat_requires_cover_before_request() {
    let wx = WeChat::new();
    assert_eq!(
        wx.create_draft(transform::markdown(&content()), &Config::new(), None)
            .await
            .unwrap_err()
            .code,
        "validation"
    );
}
#[tokio::test]
async fn generic_rest_headers_method_and_json_dispatch() {
    let server = MockServer::start().await;
    let cfg = config(
        json!({"endpoint":format!("{}/articles",server.uri()),"method":"PATCH","headers":"{\"X-Custom\":\"yes\"}","token":"test-token","template":"{\"title\":\"{{title}}\",\"action\":\"{{action}}\"}"}),
    );
    Mock::given(method("HEAD"))
        .and(path("/articles"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    GenericRest::new().test_connection(&cfg).await.unwrap();
    Mock::given(method("PATCH"))
        .and(path("/articles"))
        .and(header("authorization", "Bearer test-token"))
        .and(header("x-custom", "yes"))
        .and(body_json(
            json!({"title":content().title,"action":"publish"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":"rest-42"})))
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(
        Registry::builtin()
            .dispatch(
                "generic-rest",
                Action::Publish,
                &content(),
                &cfg,
                None,
                None
            )
            .await
            .unwrap()
            .id,
        "rest-42"
    );
}
#[tokio::test]
async fn readable_errors_do_not_expose_credentials() {
    let server = MockServer::start().await;
    for (status, code) in [
        (401, "authentication"),
        (403, "authentication"),
        (429, "rate_limit"),
        (500, "api"),
    ] {
        Mock::given(path(format!("/{status}")))
            .respond_with(ResponseTemplate::new(status).set_body_string("secret-token body"))
            .mount(&server)
            .await;
        let response = core::client()
            .get(format!(
                "{}/{status}?access_token=secret-token",
                server.uri()
            ))
            .send()
            .await
            .unwrap();
        let error = core::json_response(response).await.unwrap_err();
        assert_eq!(error.code, code);
        assert!(!error.message.contains("secret-token"));
    }
    Mock::given(path("/wx"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"errcode":40164,"errmsg":"secret-token"})),
        )
        .mount(&server)
        .await;
    let response = core::client()
        .get(format!("{}/wx", server.uri()))
        .send()
        .await
        .unwrap();
    let error = core::json_response(response).await.unwrap_err();
    assert!(error.message.contains("白名单"));
    assert!(!error.message.contains("secret-token"));
}
#[test]
fn endpoints_require_tls_and_no_credentials() {
    assert!(core::endpoint("https://example.com").is_ok());
    assert!(core::endpoint("http://127.0.0.1:1234").is_ok());
    assert!(core::endpoint("http://example.com").is_err());
    assert!(core::endpoint("https://user:password@example.com").is_err());
}
#[test]
fn local_windows_images_survive_both_transforms() {
    let mut c = content();
    c.body = "![pic](C:/images/body.png)\n\n![file](file:///C:/images/cover.png)".into();
    let mut p = transform::markdown(&c);
    assert!(p.html.contains("src=\"C:/images/body.png\""));
    p.html = publishing_workbench::extensions::wechat::compatible_html(&p.html);
    transform::replace_image(
        &mut p,
        "C:/images/body.png",
        "https://cdn.example.com/image.png",
    );
    assert!(p.html.contains("src=\"https://cdn.example.com/image.png\""));
    assert!(p.html.contains("file:///C:/images/cover.png"));
    assert_eq!(
        transform::image_sources("<img src=images/unquoted.png>"),
        vec!["images/unquoted.png"]
    );
    assert!(transform::image_sources("<script><img src='private.png'></script>").is_empty());
}
#[tokio::test]
async fn extensionless_remote_images_and_size_guard() {
    let server = MockServer::start().await;
    Mock::given(path("/asset"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "image/png")
                .set_body_bytes([1, 2, 3]),
        )
        .mount(&server)
        .await;
    let asset = transform::load_asset(&core::client(), &format!("{}/asset", server.uri()), None)
        .await
        .unwrap();
    assert_eq!(asset.mime, "image/png");
    assert_eq!(asset.name, "image.png");
    Mock::given(path("/large.png"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![0; 10 * 1024 * 1024 + 1]))
        .mount(&server)
        .await;
    assert_eq!(
        transform::load_asset(
            &core::client(),
            &format!("{}/large.png", server.uri()),
            None
        )
        .await
        .unwrap_err()
        .code,
        "asset"
    );
}
#[tokio::test]
async fn generic_rejects_local_assets_and_accepts_no_content() {
    let generic = GenericRest::new();
    let mut c = content();
    c.body = "![pic](image.png)".into();
    assert_eq!(
        generic
            .publish(transform::markdown(&c), &Config::new(), None)
            .await
            .unwrap_err()
            .code,
        "unsupported"
    );
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/articles"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    let cfg = config(
        json!({"endpoint":format!("{}/articles",server.uri()),"template":"{\"title\":\"{{title}}\"}"}),
    );
    assert_eq!(
        generic
            .publish(transform::markdown(&content()), &cfg, None)
            .await
            .unwrap()
            .status,
        "accepted"
    );
}
#[tokio::test]
async fn local_image_resolution_and_size_validation() {
    let directory =
        std::env::temp_dir().join(format!("publishing-workbench-test-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("image.png"), [1, 2, 3]).unwrap();
    let asset = transform::load_asset(&core::client(), "image.png", Some(&directory))
        .await
        .unwrap();
    assert_eq!(asset.mime, "image/png");
    assert_eq!(asset.bytes, vec![1, 2, 3]);
    assert!(transform::load_asset(&core::client(), "image.png", None)
        .await
        .is_err());
    std::fs::remove_file(directory.join("image.png")).unwrap();
    std::fs::remove_dir(directory).unwrap();
}
