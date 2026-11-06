//! Bulk Data output manifests and their pages.
//!
//! A manifest has the shape of the configured IG version. With
//! `allowPartialManifests` the files are spread over numbered pages kept next to
//! the output: a page is published once it is full and only ever changes by gaining
//! its `next` link, which is the one change the IG allows after a client has seen it.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde_json::{Map, Value, json};
use tokio::fs;
use uuid::Uuid;

use super::NDJSON_CONTENT_TYPE;
use crate::config::BulkSpecVersion;

/// Canonical of the logical model STU 4 manifests declare in `manifestType`.
pub(crate) const BULK_DATA_MANIFEST: &str =
    "http://hl7.org/fhir/uv/bulkdata/StructureDefinition/BulkDataManifest";

/// Fields every page of one manifest shares.
#[derive(Debug, Clone)]
pub(crate) struct ManifestHeader {
    pub spec: BulkSpecVersion,
    pub transaction_time: DateTime<Utc>,
    pub request: String,
    pub requires_access_token: bool,
    pub organized_by: Option<String>,
}

/// One file of a manifest.
#[derive(Debug, Clone, Default)]
pub(crate) struct ManifestFile {
    /// Absent for files organized by a resource type, which hold many types.
    pub resource_type: Option<String>,
    pub url: String,
    pub count: usize,
    pub bytes: u64,
    /// `(severity, count)` of the OperationOutcomes in an outcome file.
    pub severities: Vec<(String, usize)>,
}

impl ManifestFile {
    /// The file as a manifest array item. `fileSize` and `countSeverity` exist from STU 4 on.
    pub(crate) fn item(&self, spec: BulkSpecVersion) -> Value {
        let mut item = Map::new();
        if let Some(resource_type) = &self.resource_type {
            item.insert("type".into(), json!(resource_type));
        }
        item.insert("url".into(), json!(self.url));
        item.insert("count".into(), json!(self.count));
        if spec == BulkSpecVersion::Stu4 {
            item.insert("fileSize".into(), json!(self.bytes));
            if !self.severities.is_empty() {
                let counts: Vec<_> = self
                    .severities
                    .iter()
                    .map(|(code, count)| json!({"code": code, "count": count}))
                    .collect();
                item.insert("countSeverity".into(), json!(counts));
            }
        }
        Value::Object(item)
    }
}

impl ManifestHeader {
    /// One manifest document. STU 3 requires `request` and `error`; STU 4 has
    /// neither, and carries the messages in `outcome`.
    pub(crate) fn document(
        &self,
        output: &[ManifestFile],
        deleted: &[ManifestFile],
        outcome: &[ManifestFile],
    ) -> Value {
        let items = |files: &[ManifestFile]| {
            Value::Array(files.iter().map(|file| file.item(self.spec)).collect())
        };
        let mut manifest = Map::new();
        if self.spec == BulkSpecVersion::Stu4 {
            manifest.insert("manifestType".into(), json!(BULK_DATA_MANIFEST));
        }
        manifest.insert(
            "transactionTime".into(),
            json!(self.transaction_time.to_rfc3339()),
        );
        if self.spec == BulkSpecVersion::Stu3 {
            manifest.insert("request".into(), json!(self.request));
        }
        manifest.insert(
            "requiresAccessToken".into(),
            json!(self.requires_access_token),
        );
        if self.spec == BulkSpecVersion::Stu4 {
            manifest.insert("outputFormat".into(), json!(NDJSON_CONTENT_TYPE));
        }
        if let Some(organized_by) = &self.organized_by {
            manifest.insert("outputOrganizedBy".into(), json!(organized_by));
        }
        manifest.insert("output".into(), items(output));
        if !deleted.is_empty() {
            manifest.insert("deleted".into(), items(deleted));
        }
        let messages = match self.spec {
            BulkSpecVersion::Stu3 => "error",
            BulkSpecVersion::Stu4 => "outcome",
        };
        manifest.insert(messages.into(), items(outcome));
        Value::Object(manifest)
    }
}

fn pages_dir(export_path: &str, job_id: Uuid) -> PathBuf {
    Path::new(export_path)
        .join(job_id.to_string())
        .join("manifest")
}

fn page_path(dir: &Path, page: usize) -> PathBuf {
    dir.join(format!("{page}.json"))
}

/// Replaces a page in one step, so a poll never sees half of it.
async fn write_page(dir: &Path, page: usize, document: &Value) -> Result<(), String> {
    let path = page_path(dir, page);
    let staged = dir.join(format!("{page}.json.tmp"));
    let bytes = serde_json::to_vec(document).map_err(|error| error.to_string())?;
    fs::write(&staged, bytes)
        .await
        .map_err(|error| format!("Cannot write manifest page: {error}"))?;
    fs::rename(&staged, &path)
        .await
        .map_err(|error| format!("Cannot publish manifest page: {error}"))
}

/// Writes manifest pages while an export runs.
pub(crate) struct ManifestPages {
    dir: PathBuf,
    header: ManifestHeader,
    per_page: usize,
    /// `{status url}/manifest`, the parent of every page's URL.
    pages_url: String,
    pending: Vec<ManifestFile>,
    published: usize,
    last: Option<Value>,
}

impl ManifestPages {
    pub(crate) async fn new(
        export_path: &str,
        job_id: Uuid,
        header: ManifestHeader,
        per_page: usize,
        status_url: &str,
    ) -> Result<Self, String> {
        let dir = pages_dir(export_path, job_id);
        // A client may hold pages of an earlier attempt, whose files this attempt
        // would repeat under other names. Pages are immutable, so the job fails and
        // the client re-runs it, as the IG describes for a non-transient error.
        if fs::try_exists(page_path(&dir, 1)).await.unwrap_or(false) {
            return Err("The export was interrupted after publishing a partial manifest".into());
        }
        fs::create_dir_all(&dir)
            .await
            .map_err(|error| format!("Cannot create manifest directory: {error}"))?;
        Ok(Self {
            dir,
            header,
            per_page: per_page.max(1),
            pages_url: format!("{status_url}/manifest"),
            pending: Vec::new(),
            published: 0,
            last: None,
        })
    }

    pub(crate) async fn add(&mut self, file: ManifestFile) -> Result<(), String> {
        self.pending.push(file);
        if self.pending.len() >= self.per_page {
            self.publish(&[], &[]).await?;
        }
        Ok(())
    }

    async fn publish(
        &mut self,
        deleted: &[ManifestFile],
        outcome: &[ManifestFile],
    ) -> Result<(), String> {
        let document = self.header.document(&self.pending, deleted, outcome);
        let page = self.published + 1;
        write_page(&self.dir, page, &document).await?;
        if let Some(mut previous) = self.last.take() {
            previous["link"] = json!([{
                "relation": "next",
                "url": format!("{}/{page}", self.pages_url),
            }]);
            write_page(&self.dir, self.published, &previous).await?;
        }
        self.pending.clear();
        self.published = page;
        self.last = Some(document);
        Ok(())
    }

    /// Publishes what is left and returns the first page, the job's result.
    pub(crate) async fn finish(
        mut self,
        deleted: &[ManifestFile],
        outcome: &[ManifestFile],
    ) -> Result<Value, String> {
        if self.published == 0
            || !self.pending.is_empty()
            || !deleted.is_empty()
            || !outcome.is_empty()
        {
            self.publish(deleted, outcome).await?;
        }
        read_page_at(&self.dir, 1)
            .await
            .ok_or_else(|| "Cannot read the first manifest page".to_owned())
    }
}

async fn read_page_at(dir: &Path, page: usize) -> Option<Value> {
    let bytes = fs::read(page_path(dir, page)).await.ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// A published page, `None` when it does not exist (yet).
pub(crate) async fn read_page(export_path: &str, job_id: Uuid, page: usize) -> Option<Value> {
    read_page_at(&pages_dir(export_path, job_id), page).await
}

/// Files a client may already hold a link to, across every published page.
pub(crate) async fn published_urls(export_path: &str, job_id: Uuid) -> HashSet<String> {
    let dir = pages_dir(export_path, job_id);
    let mut urls = HashSet::new();
    let mut page = 1;
    while let Some(document) = read_page_at(&dir, page).await {
        urls.extend(file_urls(&document).map(str::to_owned));
        page += 1;
    }
    urls
}

/// Every file a manifest document lists.
pub(crate) fn file_urls(document: &Value) -> impl Iterator<Item = &str> {
    ["output", "error", "outcome", "deleted"]
        .into_iter()
        .filter_map(|field| document[field].as_array())
        .flatten()
        .filter_map(|file| file["url"].as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(spec: BulkSpecVersion) -> ManifestHeader {
        ManifestHeader {
            spec,
            transaction_time: Utc::now(),
            request: "https://example.org/fhir/$export".into(),
            requires_access_token: true,
            organized_by: None,
        }
    }

    fn file(name: &str) -> ManifestFile {
        ManifestFile {
            resource_type: Some("Patient".into()),
            url: format!("https://example.org/{name}"),
            count: 2,
            bytes: 10,
            severities: Vec::new(),
        }
    }

    #[test]
    fn each_version_has_exactly_its_own_fields() {
        let files = [file("a")];
        let stu3 = header(BulkSpecVersion::Stu3).document(&files, &[], &[]);
        assert!(stu3["request"].is_string());
        assert_eq!(stu3["error"], json!([]));
        assert!(stu3.get("outcome").is_none());
        assert!(stu3.get("manifestType").is_none());
        assert!(stu3["output"][0].get("fileSize").is_none());

        let stu4 = header(BulkSpecVersion::Stu4).document(&files, &[], &[]);
        assert!(stu4.get("request").is_none());
        assert!(stu4.get("error").is_none());
        assert_eq!(stu4["outcome"], json!([]));
        assert_eq!(stu4["manifestType"], BULK_DATA_MANIFEST);
        assert_eq!(stu4["outputFormat"], NDJSON_CONTENT_TYPE);
        assert_eq!(stu4["output"][0]["fileSize"], 10);
    }

    #[tokio::test]
    async fn pages_keep_their_files_and_only_gain_a_next_link() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().to_str().unwrap();
        let job = Uuid::new_v4();
        let mut pages = ManifestPages::new(
            path,
            job,
            header(BulkSpecVersion::Stu4),
            1,
            "https://example.org/fhir/_async-status/1",
        )
        .await
        .unwrap();
        pages.add(file("a")).await.unwrap();
        let first = read_page(path, job, 1).await.unwrap();
        assert!(first.get("link").is_none(), "no later page exists yet");

        pages.add(file("b")).await.unwrap();
        let mut linked = read_page(path, job, 1).await.unwrap();
        assert_eq!(
            linked["link"],
            json!([{"relation":"next","url":"https://example.org/fhir/_async-status/1/manifest/2"}])
        );
        linked.as_object_mut().unwrap().remove("link");
        assert_eq!(linked, first);

        let last = [file("outcome")];
        let result = pages.finish(&[], &last).await.unwrap();
        assert_eq!(result["output"][0]["url"], "https://example.org/a");
        let tail = read_page(path, job, 3).await.unwrap();
        assert_eq!(tail["output"], json!([]));
        assert_eq!(tail["outcome"][0]["url"], "https://example.org/outcome");
        assert_eq!(
            tail["transactionTime"], first["transactionTime"],
            "pages share everything but their file lists"
        );
        let urls = published_urls(path, job).await;
        assert_eq!(urls.len(), 3);
    }

    #[tokio::test]
    async fn a_restarted_attempt_cannot_rewrite_published_pages() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().to_str().unwrap();
        let job = Uuid::new_v4();
        let open = || {
            ManifestPages::new(
                path,
                job,
                header(BulkSpecVersion::Stu3),
                1,
                "https://example.org/s",
            )
        };
        let mut pages = open().await.unwrap();
        pages.add(file("a")).await.unwrap();
        assert!(open().await.is_err());
    }
}
