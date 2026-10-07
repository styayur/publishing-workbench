"""Windows release WebView2 + real Tauri IPC + Rust HTTP smoke test.
Requires Python Playwright and a fresh application profile. No cloud requests.
The test saves a temporary local REST destination, then resets only its own profile.
"""
import json
import re
import os
import socket
import subprocess
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.request import urlopen
from playwright.sync_api import sync_playwright, expect

ROOT = Path(__file__).resolve().parents[1]
import tempfile
PROFILE_DIR=Path(tempfile.mkdtemp(prefix='workbench-native-v02-'))
PROFILE=PROFILE_DIR/'workbench.sqlite3'

payloads = []
class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass
    def do_HEAD(self):
        self.send_response(200)
        self.end_headers()
    def do_PUT(self):
        self.do_POST()
    def do_POST(self):
        payloads.append(json.loads(self.rfile.read(int(self.headers['Content-Length']))))
        data = json.dumps({'id':'native-smoke-42'}).encode()
        self.send_response(201)
        self.send_header('Content-Type','application/json')
        self.send_header('Content-Length',str(len(data)))
        self.end_headers()
        self.wfile.write(data)

server = ThreadingHTTPServer(('127.0.0.1',0),Handler)
threading.Thread(target=server.serve_forever,daemon=True).start()
with socket.socket() as s:
    s.bind(('127.0.0.1',0))
    debug_port=s.getsockname()[1]
env={**os.environ,'WORKBENCH_DATA_DIR':str(PROFILE_DIR),'WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS':f'--remote-debugging-port={debug_port} --remote-debugging-address=127.0.0.1'}
startup=subprocess.STARTUPINFO()
startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
startup.wShowWindow=0
process=subprocess.Popen([str(ROOT/'src-tauri/target/release/publishing-workbench.exe')],env=env,startupinfo=startup)
try:
    for _ in range(100):
        if process.poll() is not None:
            raise RuntimeError(f'Desktop exited during startup: {process.returncode}')
        try:
            with urlopen(f'http://127.0.0.1:{debug_port}/json/version',timeout=1) as r:
                if r.status==200: break
        except Exception:
            time.sleep(.2)
    else:
        raise RuntimeError('WebView2 debug endpoint not available')
    with sync_playwright() as p:
        browser=p.chromium.connect_over_cdp(f'http://127.0.0.1:{debug_port}')
        page=browser.contexts[0].pages[0]
        page.wait_for_load_state('networkidle')
        errors=[]
        page.on('pageerror',lambda e:errors.append(str(e)))
        expect(page.get_by_role('heading',name='Editor',exact=True)).to_be_visible()
        page.get_by_label('文章标题').fill('Native smoke article')
        page.get_by_label('正文 Markdown').fill('# Native title\n\n**真实 Rust 转换**')
        page.get_by_role('button',name='保存本地',exact=True).click()
        expect(page.get_by_text('已保存到本机')).to_be_visible()
        # Local image preview and stable source-file reimport use real Rust IPC.
        import base64
        image=PROFILE_DIR/'preview.png'
        image.write_bytes(base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aR1sAAAAASUVORK5CYII='))
        local=page.evaluate("() => window.__TAURI_INTERNALS__.invoke('load_workspace')")['content']
        local['body']='![local]('+image.as_posix()+')'
        preview=page.evaluate("c => window.__TAURI_INTERNALS__.invoke('preview',{content:c,id:null})",local)
        assert 'data:image/png;base64,' in preview['html'],preview
        source=PROFILE_DIR/'source.md';source.write_text('# Source\n\nImported',encoding='utf-8')
        first=page.evaluate("v=>window.__TAURI_INTERNALS__.invoke('import_file',v)",{'path':str(source),'content':local})
        local['article_id']='142af0ae-87b0-4e0b-91a4-d7ed11d9c8d9'
        second=page.evaluate("v=>window.__TAURI_INTERNALS__.invoke('import_file',v)",{'path':str(source),'content':local})
        assert first['article_id']==second['article_id']
        gardens=page.evaluate("()=>window.__TAURI_INTERNALS__.invoke('detect_gardens')")
        assert any('styayur.co.uk' in g['label'] for g in gardens),gardens
        page.get_by_role('button',name='Extensions',exact=True).click()
        generic=page.locator('section.extension').filter(has=page.get_by_role('heading',name='Generic REST v0.2.1'))
        generic.get_by_label('Update endpoint（可选，{{remote_id}}）').fill(f'http://127.0.0.1:{server.server_port}/articles/{{{{remote_id}}}}')
        generic.get_by_label(re.compile(r'^Endpoint')).fill(f'http://127.0.0.1:{server.server_port}/articles')
        generic.get_by_role('button',name='保存配置',exact=True).click()
        expect(page.get_by_text('配置已保存，请重新测试连接')).to_be_visible()
        generic.get_by_role('button',name='测试已保存的连接',exact=True).click()
        expect(generic.get_by_text('已连接',exact=True)).to_be_visible()
        page.get_by_role('button',name='Preview',exact=True).click()
        expect(page.frame_locator('iframe').get_by_role('heading',name='Native title')).to_be_visible()
        page.get_by_role('button',name='Publish',exact=True).click()
        page.get_by_label('选择 Generic REST',exact=True).check()
        page.get_by_role('button',name='执行所选操作',exact=True).click()
        expect(page.locator('.targets').get_by_text('create · success',exact=True)).to_be_visible()
        assert len(payloads)==1,payloads
        assert payloads[0]['title']=='Native smoke article'
        assert payloads[0]['action']=='createDraft'
        assert '<strong>真实 Rust 转换</strong>' in payloads[0]['content']
        publication=page.evaluate("() => window.__TAURI_INTERNALS__.invoke('publication_state',{articleId:document.querySelector('.publish-summary small').textContent})")
        assert publication['jobs'][0]['receipt']['id']=='native-smoke-42'
        # The binary must include transform version in its immutable content hash.
        import hashlib
        job=publication['jobs'][0]
        ws=page.evaluate("()=>window.__TAURI_INTERNALS__.invoke('load_workspace')")
        material={'content':job['content'],'action':job['action'],'transform_version':'0.2.1','assets':[],'config':ws['extensions']['generic-rest']}
        assert job['content_hash']==hashlib.sha256(json.dumps(material,ensure_ascii=False,sort_keys=True,separators=(',',':')).encode()).hexdigest()

        page.get_by_role('button',name='Editor',exact=True).click()
        page.get_by_label('正文 Markdown').fill('# Native title\n\nUpdated through the same mapping')
        page.get_by_role('button',name='Publish',exact=True).click()
        page.get_by_label('选择 Generic REST',exact=True).check()
        page.get_by_role('button',name='执行所选操作',exact=True).click()
        expect(page.locator('.targets').get_by_text('update · success',exact=True)).to_be_visible()
        assert len(payloads)==2,payloads
        assert payloads[1]['action']=='update'
        import sqlite3
        with sqlite3.connect(PROFILE) as db:
            assert db.execute('SELECT count(*) FROM remote_mappings').fetchone()[0]==1
            assert db.execute('SELECT count(*) FROM publish_jobs').fetchone()[0]==2
            saved=json.loads(db.execute("SELECT value FROM settings WHERE key='workspace'").fetchone()[0])
            assert 'token' not in saved['extensions']['generic-rest']
            assert 'headers' not in saved['extensions']['generic-rest']
        assert not errors,errors
        (ROOT/'test-results').mkdir(exist_ok=True)
        page.screenshot(path=str(ROOT/'test-results/native-publish.png'),full_page=True)
        print('Native smoke passed: release startup, real IPC, content persistence, Rust HTML preview, schema config, HEAD connection, REST draft request, create → update mapping, SQLite journal, no credential fields on disk')
        browser.close()
finally:
    process.terminate()
    process.wait(timeout=15)
    server.shutdown()
    print(f'Isolated native test profile: {PROFILE_DIR}')
