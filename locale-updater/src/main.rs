use aws_sdk_s3::primitives::ByteStream;
use polib::catalog::Catalog;
use polib::message::Message;
use polib::metadata::CatalogMetadata;
use polib::mo_file;
use quick_xml::de::from_str;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;

const PATCHES_CHAIN_URL: &str = "https://wgus-eu.wargaming.net/api/v1/patches_chain/?game_id=WOWS.WW.PRODUCTION&protocol_version=1.11&metadata_version=20251121135024&metadata_protocol_version=7.10&client_type=high&lang=ZH_SG&chain_id=f21&game_installation=false&gc_publisher=wargaming&client_current_version=0&hotfix_current_version=0&locale_current_version=0&sdcontent_current_version=0";

const MO_MAGIC: u32 = 0x950412de;

// ---------------------------------------------------------------------------
// XML deserialization structures
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename = "protocol")]
struct PatchesChainRoot {
    #[serde(rename = "patches_chain", default)]
    patches_chain: PatchesChain,
}

#[derive(Debug, Deserialize, Default)]
struct PatchesChain {
    #[serde(rename = "patch", default)]
    patch: Vec<Patch>,
}

#[derive(Debug, Deserialize)]
struct Patch {
    #[serde(rename = "files", default)]
    files: Vec<FilesElement>,
    #[serde(rename = "torrent", default)]
    torrent: Vec<Torrent>,
}

#[derive(Debug, Deserialize)]
struct FilesElement {
    #[serde(rename = "file", default)]
    file: Vec<FileEntry>,
}

#[derive(Debug, Deserialize)]
struct FileEntry {
    #[serde(rename = "name")]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Torrent {
    #[serde(rename = "urls")]
    urls: Urls,
}

#[derive(Debug, Deserialize)]
struct Urls {
    #[serde(rename = "url", default)]
    url: Vec<String>,
}

// ---------------------------------------------------------------------------
// MO parser (pure binary, preserves NUL bytes)
// ---------------------------------------------------------------------------

struct MoFile {
    entries: BTreeMap<Vec<u8>, Vec<u8>>,
}

impl MoFile {
    fn parse(data: &[u8]) -> Result<Self, String> {
        if data.len() < 28 {
            return Err("MO file too short for header".into());
        }
        if read_u32_le(data, 0) != MO_MAGIC {
            return Err(format!("Invalid MO magic: 0x{:08x}", read_u32_le(data, 0)));
        }
        let num = read_u32_le(data, 8) as usize;
        let orig_off = read_u32_le(data, 12) as usize;
        let trans_off = read_u32_le(data, 16) as usize;

        let mut entries = BTreeMap::new();
        for i in 0..num {
            let olen = read_u32_le(data, orig_off + i * 8) as usize;
            let ooff = read_u32_le(data, orig_off + i * 8 + 4) as usize;
            let tlen = read_u32_le(data, trans_off + i * 8) as usize;
            let toff = read_u32_le(data, trans_off + i * 8 + 4) as usize;
            entries.insert(
                data[ooff..ooff + olen].to_vec(),
                data[toff..toff + tlen].to_vec(),
            );
        }
        Ok(MoFile { entries })
    }

    fn filter_eventum(&self) -> MoFile {
        let mut out = BTreeMap::new();
        for (k, v) in &self.entries {
            if !String::from_utf8_lossy(k).contains("EVENTUM") {
                out.insert(k.clone(), v.clone());
            }
        }
        MoFile { entries: out }
    }

    fn to_bytes(&self) -> Result<Vec<u8>, String> {
        let mut metadata = CatalogMetadata::new();
        let mut catalog = Catalog::new(metadata.clone());

        for (msgid, msgstr) in &self.entries {
            let msgid_parts = nul_split(msgid);
            if msgid_parts.len() == 1 && msgid_parts[0].is_empty() {
                let ms_parts = nul_split(msgstr);
                if let Some(header) = ms_parts.first() {
                    if !header.is_empty() {
                        metadata = CatalogMetadata::parse(header.as_str())
                            .map_err(|e| format!("parse header: {e}"))?;
                    }
                }
                continue;
            }

            let message = if msgid_parts.len() == 1 {
                let ms_parts = nul_split(msgstr);
                let msgstr_single = ms_parts.first().map(|s| s.as_str()).unwrap_or("");
                let mut b = Message::build_singular();
                b.with_msgid(msgid_parts[0].clone());
                b.with_msgstr(msgstr_single.to_string());
                b.done()
            } else {
                let ms_parts = nul_split(msgstr);
                let mut b = Message::build_plural();
                b.with_msgid(msgid_parts[0].clone());
                b.with_msgid_plural(msgid_parts[1].clone());
                b.with_msgstr_plural(ms_parts);
                b.done()
            };
            catalog.append_or_update(message);
        }

        catalog.metadata = metadata;

        let dir = tempfile::tempdir().map_err(|e| format!("tempdir: {e}"))?;
        let path = dir.path().join("out.mo");
        mo_file::write(&catalog, &path).map_err(|e| format!("write mo: {e}"))?;
        std::fs::read(&path).map_err(|e| format!("read temp mo: {e}"))
    }
}

fn read_u32_le(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
}

fn nul_split(data: &[u8]) -> Vec<String> {
    if data.is_empty() {
        return vec![String::new()];
    }
    let end = if data.last() == Some(&0) { data.len() - 1 } else { data.len() };
    let mut parts = Vec::new();
    let mut start = 0;
    for i in 0..end {
        if data[i] == 0 {
            parts.push(String::from_utf8_lossy(&data[start..i]).to_string());
            start = i + 1;
        }
    }
    parts.push(String::from_utf8_lossy(&data[start..end]).to_string());
    parts
}

// ---------------------------------------------------------------------------
// Patches chain fetching & dspkg URL construction
// ---------------------------------------------------------------------------

fn fetch_locale_dspkg_url() -> Result<String, String> {
    eprintln!("[1/5] Fetching patches chain...");

    let xml = reqwest::blocking::get(PATCHES_CHAIN_URL)
        .map_err(|e| format!("HTTP GET failed: {e}"))?
        .text()
        .map_err(|e| format!("read body: {e}"))?;

    let root: PatchesChainRoot =
        from_str(&xml).map_err(|e| format!("parse XML: {e}"))?;

    for patch in &root.patches_chain.patch {
        let mut file_name: Option<String> = None;
        for fe in &patch.files {
            for f in &fe.file {
                if let Some(ref n) = f.name {
                    if n.contains("locale") {
                        file_name = Some(n.clone());
                        break;
                    }
                }
            }
        }

        let file_name = match file_name {
            Some(n) => n,
            None => continue,
        };

        for torrent in &patch.torrent {
            for url in &torrent.urls.url {
                let url = url.trim();
                if url.is_empty() {
                    continue;
                }

                let dspkg_url = build_direct_url(url, &file_name)?;
                eprintln!("  locale file: {file_name}");
                eprintln!("  direct URL : {dspkg_url}");
                return Ok(dspkg_url);
            }
        }
    }

    Err("no locale dspkg found in patches chain".into())
}

fn build_direct_url(torrent_url: &str, file_name: &str) -> Result<String, String> {
    if let Some(pos) = torrent_url.find("patches/") {
        let base = &torrent_url[..pos + "patches/".len()];
        return Ok(format!("{base}{file_name}"));
    }
    Err("cannot extract base path from torrent URL".into())
}

// ---------------------------------------------------------------------------
// Download & extract
// ---------------------------------------------------------------------------

fn download_and_extract_mo(dspkg_url: &str) -> Result<Vec<u8>, String> {
    eprintln!("[2/5] Downloading dspkg...");

    let mut resp = reqwest::blocking::get(dspkg_url)
        .map_err(|e| format!("download dspkg: {e}"))?;
    let total = resp.content_length().unwrap_or(0);

    let mut buf = Vec::new();
    let mut downloaded: u64 = 0;
    loop {
        let mut chunk = vec![0u8; 65536];
        let n = resp.read(&mut chunk).map_err(|e| format!("read dspkg: {e}"))?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        downloaded += n as u64;
        if total > 0 && downloaded % (total.max(1) / 10) < 65536 {
            eprintln!("  {}/{} bytes ({:.0}%)", downloaded, total,
                downloaded as f64 / total as f64 * 100.0);
        }
    }
    eprintln!("  downloaded {} bytes", downloaded);

    eprintln!("[3/5] Extracting global.mo from dspkg...");

    let reader = std::io::Cursor::new(&buf);
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|e| format!("open zip: {e}"))?;

    let mut best: Option<(u64, String)> = None;
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|e| format!("zip entry: {e}"))?;
        let name = entry.name().to_string();
        if name.ends_with("texts/zh_sg/LC_MESSAGES/global.mo") {
            let num = name.split('/')
                .find_map(|p| p.parse::<u64>().ok())
                .unwrap_or(0);
            match &best {
                Some((n, _)) if num > *n => best = Some((num, name)),
                None => best = Some((num, name)),
                _ => {}
            }
        }
    }

    let entry_name = best.ok_or("global.mo not found in dspkg")?.1;
    eprintln!("  extracting: {entry_name}");

    let mut entry = archive.by_name(&entry_name)
        .map_err(|e| format!("extract {entry_name}: {e}"))?;
    let mut mo_data = Vec::new();
    entry.read_to_end(&mut mo_data)
        .map_err(|e| format!("read mo: {e}"))?;

    Ok(mo_data)
}

// ---------------------------------------------------------------------------
// Process MO (filter EVENTUM)
// ---------------------------------------------------------------------------

fn process_mo(data: &[u8]) -> Result<Vec<u8>, String> {
    eprintln!("[4/5] Processing MO file...");

    let mo = MoFile::parse(data)?;
    let filtered = mo.filter_eventum();
    let output = filtered.to_bytes()?;

    let hash = hex::encode({
        let mut h = Sha256::new();
        h.update(&output);
        h.finalize()
    });
    eprintln!("  output size: {} bytes", output.len());
    eprintln!("  sha256: {hash}");

    Ok(output)
}

// ---------------------------------------------------------------------------
// Upload to R2
// ---------------------------------------------------------------------------

async fn upload_to_r2(data: &[u8]) -> Result<(), String> {
    eprintln!("[5/5] Uploading to R2...");

    let bucket = std::env::var("R2_BUCKET_NAME")
        .map_err(|_| "R2_BUCKET_NAME not set".to_string())?;
    let endpoint = std::env::var("R2_ENDPOINT_URL")
        .map_err(|_| "R2_ENDPOINT_URL not set".to_string())?;
    let region = std::env::var("AWS_REGION").unwrap_or_else(|_| "auto".into());

    let config = aws_config::from_env()
        .endpoint_url(&endpoint)
        .region(aws_config::Region::new(region))
        .load()
        .await;

    let client = aws_sdk_s3::Client::new(&config);
    let key = "i18n/zh_cn360/global.mo";

    client
        .put_object()
        .bucket(&bucket)
        .key(key)
        .body(ByteStream::from(data.to_vec()))
        .content_type("application/octet-stream")
        .cache_control("public, max-age=3600")
        .send()
        .await
        .map_err(|e| format!("upload to R2 failed: {e}"))?;

    eprintln!("  uploaded to {bucket}/{key}");
    Ok(())
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("ERROR: {e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let dspkg_url = fetch_locale_dspkg_url()?;
    let mo_data = download_and_extract_mo(&dspkg_url)?;
    let processed = process_mo(&mo_data)?;
    upload_to_r2(&processed).await?;

    eprintln!("Done!");
    Ok(())
}
