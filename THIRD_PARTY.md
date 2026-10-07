# Third-party materials

Dependencies retain their original licenses; AGPL does not relicense them. No third-party images, fonts, dataset, Digital Garden source or private site content is bundled by this repository.

Major runtime components: Tauri / tauri-runtime (MIT OR Apache-2.0), React / React DOM (MIT), reqwest (MIT OR Apache-2.0), Tokio (MIT), serde / serde_json (MIT OR Apache-2.0), rusqlite (MIT), SQLite (public domain), sha2 (MIT OR Apache-2.0), uuid (MIT OR Apache-2.0), pulldown-cmark (MIT), ammonia (MIT OR Apache-2.0), keyring (MIT OR Apache-2.0), fs2 (MIT OR Apache-2.0), chrono (MIT OR Apache-2.0), marked (MIT), DOMPurify (Apache-2.0 OR MPL-2.0), yaml (ISC).

Cargo.lock and package-lock.json pin the resolved dependency graph. Windows WebView2 is an externally installed Microsoft runtime under its own terms. Development/test/build dependencies also retain their package terms.

The release contains generated `third-party-licenses.json`, listing resolved Cargo and npm package license metadata and repositories. It also contains `dependency-license-texts.zip`, collecting available package LICENSE/COPYING files from installed Cargo and npm sources. These are informational inventories, not substitutions for upstream license terms. Consult each upstream package for authoritative notices and source.
