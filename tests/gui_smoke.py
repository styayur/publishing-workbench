"""GUI smoke tests using actual Rust manifests and a fake Tauri IPC boundary.
Run with Vite on 127.0.0.1:1420. Never sends requests to real publishing APIs.
"""
import json
from pathlib import Path
from playwright.sync_api import sync_playwright, expect

ROOT = Path(__file__).resolve().parents[1]
MANIFESTS = json.loads((ROOT / "tests/manifests.fixture.json").read_text(encoding="utf-8-sig"))
OUTPUT = ROOT / "test-results"
OUTPUT.mkdir(exist_ok=True)
bridge = """
globalThis.isTauri = true;
const manifests = MANIFEST_DATA;
const workspace = {content:{article_id:'8c975b14-9875-43c5-8cbb-243d20671b16',format:'markdown',source_path:null,title:'GUI test article',slug:'gui-test',summary:'Summary',body:'# GUI test\\n\\n**Hello**',cover:null,authors:[],tags:[],metadata:{}},asset_directory:'',extensions:{},history:[]};
globalThis.testCalls=[];
globalThis.__TAURI_INTERNALS__ = {invoke:async (cmd,args={})=>{
  globalThis.testCalls.push({cmd,args});
  if(cmd==='manifests')return manifests;
  if(cmd==='load_workspace')return structuredClone(workspace);
  if(cmd==='save_content'){workspace.content=args.content;workspace.asset_directory=args.assetDirectory;return;}
  if(cmd==='save_config'){workspace.extensions[args.id]=Object.fromEntries(Object.entries(args.config).filter(([k])=>!manifests.find(m=>m.id===args.id).schema.properties[k].secret));return;}
  if(cmd==='test_connection')return;
  if(cmd==='detect_gardens')return [];
  if(cmd==='publication_state')return {jobs:workspace.history,mappings:[],plans:manifests.map(m=>({extension_id:m.id,target_id:m.id,operation:'create',remote_id:null})),orphans:[]};
  if(cmd==='preview')return {content:args.content,html:'<h1>GUI test</h1><p><strong>Hello</strong></p><script>alert(1)</script>',warnings:[]};
  if(cmd==='dispatch'){
    const error=args.id==='wechat'?{code:'authentication',message:'模拟认证失败'}:null;
    const receipt=error?null:{id:'42',url:null,status:args.action==='createDraft'?'draft':'publish'};
    const job={job_id:crypto.randomUUID(),article_id:args.content.article_id,target_id:args.id,extension_id:args.id,content:args.content,operation:'create',action:args.action,status:error?'failed':'success',error,receipt,started_at:Math.floor(Date.now()/1000),attempts:1,assets_reused:0,assets_uploaded:0,steps:['transform','upload assets','upload cover','create/update remote','save mapping','complete'].map(name=>({name,status:error?'failed':'success',detail:''}))};
    workspace.history.push(job);
    return job;
  }
  throw new Error('Unknown IPC command '+cmd);
}};
""".replace("MANIFEST_DATA", json.dumps(MANIFESTS, ensure_ascii=False))

with sync_playwright() as p:
    browser = p.chromium.launch(headless=True)
    page = browser.new_page(viewport={"width": 1240, "height": 850})
    errors = []
    page.on("pageerror", lambda error: errors.append(str(error)))
    page.add_init_script(bridge)
    page.goto("http://127.0.0.1:1420")
    page.wait_for_load_state("networkidle")
    expect(page.get_by_label("文章标题")).to_have_value("GUI test article")
    page.screenshot(path=str(OUTPUT / "editor.png"), full_page=True)
    page.get_by_label("文章标题").fill("Updated article")
    page.get_by_role("button", name="保存本地", exact=True).click()
    expect(page.get_by_text("已保存到本机")).to_be_visible()
    page.get_by_role("button", name="Extensions", exact=True).click()
    expect(page.locator("section.extension")).to_have_count(4)
    wp = page.locator("section.extension").filter(has=page.get_by_role("heading", name="WordPress v0.2.0"))
    wp.get_by_label("站点 URL").fill("https://example.com")
    wp.get_by_label("用户名").fill("editor")
    wp.locator("input[type=password]").fill("fake-credential")
    wp.get_by_role("button", name="保存配置", exact=True).click()
    expect(page.get_by_text("配置已保存，请重新测试连接")).to_be_visible()
    expect(wp.locator("input[type=password]")).to_have_value("")
    generic = page.locator("section.extension").filter(has=page.get_by_role("heading", name="Generic REST v0.2.0"))
    expect(generic.locator("textarea").first).to_have_value("")
    page.screenshot(path=str(OUTPUT / "extensions.png"), full_page=True)
    page.get_by_role("button", name="Publish", exact=True).click()
    expect(page.get_by_label("WeChat Official Account 操作").locator("option")).to_have_count(1)
    expect(page.get_by_label("WeChat Official Account 操作")).to_have_value("createDraft")
    expect(page.get_by_label("WordPress 操作").locator("option")).to_have_count(2)
    page.get_by_label("选择 WordPress", exact=True).check()
    page.get_by_label("选择 WeChat Official Account", exact=True).check()
    expect(page.get_by_role("button", name="执行所选操作")).to_be_disabled()
    for target in ["WordPress", "WeChat Official Account"]:
        row = page.locator("section.target").filter(has=page.get_by_role("heading", name=target, exact=True))
        row.get_by_role("button", name="测试连接", exact=True).click()
        expect(row.get_by_text("已连接", exact=True)).to_be_visible()
    page.get_by_role("button", name="执行所选操作").click()
    expect(page.locator('.targets').get_by_text("create · success", exact=True)).to_be_visible()
    expect(page.get_by_text("create · failed · 模拟认证失败", exact=True)).to_be_visible()
    expect(page.locator(".history > details")).to_have_count(2)
    page.screenshot(path=str(OUTPUT / "publish.png"), full_page=True)
    page.get_by_role("button", name="Preview", exact=True).click()
    frame = page.frame_locator("iframe[title='文章预览']")
    expect(frame.get_by_role("heading", name="GUI test")).to_be_visible()
    assert page.locator("iframe").get_attribute("sandbox") == ""
    assert frame.locator("script").count() == 0
    page.get_by_role("button", name="Settings", exact=True).click()
    page.get_by_label("本地图片根目录").fill("C:/images")
    page.get_by_role("button", name="保存本地", exact=True).click()
    assert page.evaluate("testCalls.filter(c=>c.cmd==='save_content').at(-1).args.assetDirectory") == "C:/images"
    page.set_viewport_size({"width": 600, "height": 800})
    page.screenshot(path=str(OUTPUT / "responsive.png"), full_page=True)
    assert page.evaluate("document.documentElement.scrollWidth <= window.innerWidth")
    assert not errors, errors
    print("GUI smoke passed: editor/save, schema forms, secret reset, capabilities, multi-target partial failure, history, sandbox preview, settings, responsive layout; no JS errors")
    browser.close()
