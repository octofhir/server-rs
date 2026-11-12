# Temporary dependency security patches

These are unpublished source patches; the server/package release versions are unchanged.
Upstream licenses are preserved in each directory. Remove each patch when the parent
dependency accepts the corresponding fixed upstream release.

* `rusqlite`: crates.io 0.40.2, with only its two `libsqlite3-sys` dependency requirements
  changed to 0.39.1 (bundled SQLite 3.54.1). This keeps the API expected by canonical-core
  and deadpool-sqlite while addressing
  [RUSTSEC-2026-0459 / CVE-2026-119375](https://rustsec.org/advisories/RUSTSEC-2026-0459.html).
  The optional SQLCipher backend is not covered by the bundled SQLite fix and is not
  enabled by this workspace. Updating canonical-core alone is insufficient: its latest
  available release at review time is 0.2.13 and still requires rusqlite 0.40.
* `lru`: crates.io 0.16.4, with `LruCache::pop` detaching its node before invoking the
  stored key's destructor. This backports the panic-safety ordering fix described in
  [RUSTSEC-2026-0253](https://rustsec.org/advisories/RUSTSEC-2026-0253.html) without changing
  async-graphql's dependency API. The regression test exercises cache use after a key's
  destructor panics. Other lru versions in the graph remain registry dependencies.
