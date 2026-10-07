import { useEffect, useState } from 'react';
import { api, desktop } from '../api';
import type { ConfigSuggestion, Manifest } from '../models';
export function Extensions({
  manifests,
  configs,
  onSaved,
  onError,
  onTest,
  connected,
}: {
  manifests: Manifest[];
  configs: Record<string, Record<string, string>>;
  onSaved: () => Promise<void>;
  onError: (e: unknown) => void;
  onTest: (id: string) => Promise<void>;
  connected: Record<string, boolean>;
}) {
  const [drafts, setDrafts] = useState<Record<string, Record<string, string>>>(
    {},
  );
  const [clear, setClear] = useState<Record<string, string[]>>({});
  const [busy, setBusy] = useState('');
  const [suggestions, setSuggestions] = useState<ConfigSuggestion[]>([]);
  useEffect(() => {
    if (desktop)
      void api<ConfigSuggestion[]>('detect_gardens')
        .then(setSuggestions)
        .catch(onError);
  }, [onError]);
  return (
    <>
      <div className="section-heading">
        <div>
          <h2>Extensions</h2>
          <p>配置保存在本机；密钥存入系统凭据库，空白表示保留原密钥。</p>
        </div>
      </div>
      {manifests.map((m) => {
        const cfg = {
          ...Object.fromEntries(
            Object.entries(m.schema.properties).map(([k, v]) => [
              k,
              v.secret ? '' : (v.default ?? ''),
            ]),
          ),
          ...configs[m.id],
          ...drafts[m.id],
        };
        return (
          <section className="extension" key={m.id}>
            <div className="section-heading">
              <div>
                <h3>
                  {m.name} <small>v{m.version}</small>
                </h3>
                <p>{m.description}</p>
              </div>
              <span className={connected[m.id] ? 'badge success' : 'badge'}>
                {connected[m.id] ? '已连接' : '未验证'}
              </span>
            </div>
            {suggestions
              .filter((s) => s.extension_id === m.id)
              .map((s) => (
                <div className="notice" key={s.label}>
                  <strong>已探测 {s.label}</strong>
                  <p>{s.evidence}</p>
                  <button
                    onClick={() => setDrafts({ ...drafts, [m.id]: s.config })}
                  >
                    填入配置建议（仍需保存）
                  </button>
                </div>
              ))}
            <div className="capabilities">
              {m.capabilities.map((c) => (
                <span key={c}>{c}</span>
              ))}
            </div>
            <div className="fields">
              {Object.entries(m.schema.properties).map(([key, s]) => (
                <label key={key} className={s.multiline ? 'wide' : ''}>
                  {s.title}
                  {m.schema.required.includes(key) && ' *'}
                  {s.enum ? (
                    <select
                      value={cfg[key]}
                      onChange={(e) =>
                        setDrafts({
                          ...drafts,
                          [m.id]: { ...drafts[m.id], [key]: e.target.value },
                        })
                      }
                    >
                      {s.enum.map((v) => (
                        <option key={v}>{v}</option>
                      ))}
                    </select>
                  ) : s.multiline ? (
                    <textarea
                      rows={4}
                      value={cfg[key]}
                      placeholder={
                        s.secret
                          ? '已保存的值不会回显；填写后覆盖'
                          : s.placeholder
                      }
                      onChange={(e) =>
                        setDrafts({
                          ...drafts,
                          [m.id]: { ...drafts[m.id], [key]: e.target.value },
                        })
                      }
                    />
                  ) : (
                    <input
                      autoComplete="off"
                      type={s.secret ? 'password' : 'text'}
                      placeholder={
                        s.secret ? '填写后保存，空白保留' : s.placeholder
                      }
                      value={cfg[key]}
                      onChange={(e) =>
                        setDrafts({
                          ...drafts,
                          [m.id]: { ...drafts[m.id], [key]: e.target.value },
                        })
                      }
                    />
                  )}{' '}
                  {s.secret && (
                    <span className="secret-clear">
                      <input
                        type="checkbox"
                        checked={(clear[m.id] ?? []).includes(key)}
                        onChange={(e) =>
                          setClear({
                            ...clear,
                            [m.id]: e.target.checked
                              ? [...(clear[m.id] ?? []), key]
                              : (clear[m.id] ?? []).filter((k) => k !== key),
                          })
                        }
                      />
                      清除已保存的凭据
                    </span>
                  )}
                </label>
              ))}
            </div>
            <div className="actions">
              <button
                disabled={!!busy}
                onClick={async () => {
                  setBusy(m.id);
                  try {
                    await api('save_config', {
                      id: m.id,
                      config: cfg,
                      clearSecrets: clear[m.id] ?? [],
                    });
                    setDrafts({ ...drafts, [m.id]: {} });
                    setClear({ ...clear, [m.id]: [] });
                    await onSaved();
                  } catch (e) {
                    onError(e);
                  } finally {
                    setBusy('');
                  }
                }}
              >
                {busy === m.id ? '保存中…' : '保存配置'}
              </button>
              <button disabled={!!busy} onClick={() => onTest(m.id)}>
                测试已保存的连接
              </button>
            </div>
          </section>
        );
      })}
    </>
  );
}
