import DOMPurify from 'dompurify';
export function Preview({
  html,
  warnings = [],
}: {
  html: string;
  warnings?: string[];
}) {
  const safe = DOMPurify.sanitize(html);
  const document = `<!doctype html><html><head><meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src https: http: data:; style-src 'unsafe-inline'"><style>body{font:17px/1.8 Georgia,'Microsoft YaHei',serif;color:#263d4c;max-width:720px;margin:30px auto;padding:0 24px}img{max-width:100%}pre{white-space:pre-wrap;background:#edf3f6;padding:15px}table{border-collapse:collapse}td,th{border:1px solid #ccd7df;padding:8px}blockquote{border-left:3px solid #779baa;padding-left:16px}a{color:#16718a}</style></head><body>${safe}</body></html>`;
  return (
    <>
      {warnings.map((w) => (
        <p key={w} className="notice">
          {w}
        </p>
      ))}
      <iframe
        title="文章预览"
        className="article-preview"
        sandbox=""
        srcDoc={document}
      />
    </>
  );
}
