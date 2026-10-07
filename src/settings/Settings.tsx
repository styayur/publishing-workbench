export function Settings({
  directory,
  onChange,
  onExport,
}: {
  directory: string;
  onChange: (v: string) => void;
  onExport: () => void;
}) {
  return (
    <>
      <h2>Settings</h2>
      <p>所有发布请求由本机 Rust 核心发送，无需云服务。</p>
      <section className="extension">
        <label>
          本地图片根目录
          <input
            value={directory}
            onChange={(e) => onChange(e.target.value)}
            placeholder="C:/Users/you/Pictures"
          />
        </label>
        <p>
          Markdown 相对图片路径从此目录解析。绝对路径与远程 URL
          可直接使用。修改后点击顶部“保存本地”。
        </p>
        <button onClick={onExport}>导出文章 JSON</button>
      </section>
      <section className="extension">
        <h3>本地存储</h3>
        <p>
          文章、非密钥配置、最近 100 次发布结果保存在系统应用数据目录的
          workspace.json 中。密码、Token、AppSecret 和 Headers 使用系统凭据库。
        </p>
        <p>
          预览不会上传图片。实际发布时，支持素材的扩展上传图片并替换
          URL。上传或发布失败后，请检查远端是否已有素材或文章。
        </p>
      </section>
    </>
  );
}
