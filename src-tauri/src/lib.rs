mod instances;
mod mo;

use instances::{InstanceInfo, InstallationInfo};
use serde::{Deserialize, Serialize};
use tauri::Emitter;
use futures_util::StreamExt;
use std::io::Write;

#[derive(Clone, Serialize)]
struct ProgressPayload {
    message: String,
    downloaded: u64,
    total: u64,
}

fn emit_progress(app: &tauri::AppHandle, message: &str, downloaded: u64, total: u64) {
    let _ = app.emit("locale-progress", ProgressPayload {
        message: message.to_string(),
        downloaded,
        total,
    });
}

const PATCHES_CHAIN_URL: &str = "https://wgus-eu.wargaming.net/api/v1/patches_chain/?game_id=WOWS.WW.PRODUCTION&protocol_version=1.11&metadata_version=20251121135024&metadata_protocol_version=7.10&client_type=high&lang=ZH_SG&chain_id=f21&game_installation=false&gc_publisher=wargaming&client_current_version=0&hotfix_current_version=0&locale_current_version=0&sdcontent_current_version=0";

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
    file: Vec<LocaleFileEntry>,
}

#[derive(Debug, Deserialize)]
struct LocaleFileEntry {
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

fn fetch_locale_dspkg_url() -> Result<String, String> {
    let xml = reqwest::blocking::get(PATCHES_CHAIN_URL)
        .map_err(|e| format!("请求更新信息失败: {e}"))?
        .text()
        .map_err(|e| format!("读取响应失败: {e}"))?;

    let root: PatchesChainRoot =
        quick_xml::de::from_str(&xml).map_err(|e| format!("解析 XML 失败: {e}"))?;

    for patch in &root.patches_chain.patch {
        let file_name = patch.files
            .iter()
            .flat_map(|fe| &fe.file)
            .find_map(|f| f.name.as_ref().filter(|n| n.contains("locale")))
            .cloned();

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
                return build_direct_url(url, &file_name);
            }
        }
    }

    Err("未找到语言包更新".into())
}

fn build_direct_url(torrent_url: &str, file_name: &str) -> Result<String, String> {
    if let Some(pos) = torrent_url.find("patches/") {
        let base = &torrent_url[..pos + "patches/".len()];
        return Ok(format!("{base}{file_name}"));
    }
    Err("无法从 torrent URL 提取基础路径".into())
}

fn extract_mo_from_dspkg(url: &str) -> Result<Vec<u8>, String> {
    let mut rf = gc_download::remote_file::RemoteFile::new(url)
        .map_err(|e| format!("打开远程 dspkg 失败: {e}"))?;

    let entries = gc_download::sevenz::parse_archive_index(&mut rf)
        .map_err(|e| format!("解析 7z 索引失败: {e}"))?;

    let best = entries
        .iter()
        .filter(|e| e.filename.ends_with("texts/zh_sg/LC_MESSAGES/global.mo"))
        .max_by_key(|e| {
            e.filename
                .split('/')
                .find_map(|s| s.parse::<u64>().ok())
                .unwrap_or(0)
        })
        .ok_or_else(|| "未在 dspkg 中找到 global.mo".to_string())?;

    gc_download::sevenz::extract_entry(&mut rf, &best.filename)
        .map_err(|e| format!("提取 global.mo 失败: {e}"))
}

fn download_mo(app: &tauri::AppHandle) -> Result<Vec<u8>, String> {
    emit_progress(app, "正在获取更新信息...", 0, 0);

    let dspkg_url = fetch_locale_dspkg_url()?;

    emit_progress(app, "正在下载语言文件...", 0, 0);

    let mo_data = extract_mo_from_dspkg(&dspkg_url)?;

    emit_progress(app, "语言文件下载完成", 0, 0);

    Ok(mo_data)
}

#[tauri::command]
fn scan_instances() -> Result<Vec<InstanceInfo>, String> {
    instances::scan_instances()
}

#[tauri::command]
fn add_instance(path: String) -> Result<InstanceInfo, String> {
    let p = std::path::Path::new(&path);

    instances::is_valid_instance(p).map_err(|e| format!("非法的 CN360 实例路径: {}", e))?;

    let existing = instances::scan_instances().unwrap_or_default();
    let normalized = p
        .canonicalize()
        .map(|pb| pb.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.clone());

    for inst in &existing {
        if inst.path == normalized {
            return Err("实例路径已存在".to_string());
        }
    }

    instances::persist_instance_path(&normalized)?;

    let version_folders = instances::find_version_folders(p);
    let installations: Vec<InstallationInfo> = version_folders
        .iter()
        .map(|vf| {
            let (text_installed, chat_installed) = instances::check_components(p, vf);
            InstallationInfo { version_folder: vf.clone(), text_installed, chat_installed }
        })
        .collect();

    Ok(InstanceInfo {
        path: normalized,
        version_folders,
        installations,
    })
}

#[tauri::command]
fn refresh_instance(path: String) -> Result<InstanceInfo, String> {
    let p = std::path::Path::new(&path);

    instances::is_valid_instance(p)
        .map_err(|e| format!("路径不再是有效的 CN360 实例: {}", e))?;

    let version_folders = instances::find_version_folders(p);
    let installations: Vec<InstallationInfo> = version_folders
        .iter()
        .map(|vf| {
            let (text_installed, chat_installed) = instances::check_components(p, vf);
            InstallationInfo { version_folder: vf.clone(), text_installed, chat_installed }
        })
        .collect();

    Ok(InstanceInfo {
        path: p.to_string_lossy().to_string(),
        version_folders,
        installations,
    })
}

#[tauri::command]
fn install_locale_pack(
    app: tauri::AppHandle,
    instance_path: String,
    text_anticensor: bool,
    chat_anticensor: bool,
) -> Result<Vec<InstallationInfo>, String> {
    let p = std::path::Path::new(&instance_path);

    instances::is_valid_instance(p)
        .map_err(|e| format!("非法的 CN360 实例路径: {}", e))?;

    let (new_mo_data, version_folders) = if text_anticensor {
        emit_progress(&app, "正在连接服务器...", 0, 0);

        let mo_data = download_mo(&app)?;

        emit_progress(&app, "正在处理语言文件...", 0, 0);

        let mo_file = mo::MoFile::parse(&mo_data)
            .map_err(|e| format!("解析 global.mo 失败: {}", e))?;

        let filtered = mo_file.filter_eventum();
        let new_mo_data = filtered.to_bytes()
            .map_err(|e| format!("生成 global.mo 失败: {}", e))?;

        let version_folders = instances::find_version_folders(p);
        (Some(new_mo_data), version_folders)
    } else {
        (None, instances::find_version_folders(p))
    };

    let mut installations = Vec::new();

    for vf in &version_folders {
        emit_progress(&app, &format!("正在安装到版本 {} ...", vf), 0, 0);

        let vf_dir = p.join("bin").join(vf);
        let mut info_map = serde_json::Map::new();

        if let Some(ref mo_data) = new_mo_data {
            let target_dir = vf_dir
                .join("res_mods")
                .join("texts")
                .join("zh_cn")
                .join("LC_MESSAGES");
            std::fs::create_dir_all(&target_dir)
                .map_err(|e| format!("创建目录失败: {}", e))?;

            let mo_dest = target_dir.join("global.mo");
            std::fs::write(&mo_dest, mo_data)
                .map_err(|e| format!("写出 global.mo 失败: {}", e))?;

            let mo_sha256 = hex::encode({
                use sha2::{Digest, Sha256};
                let mut h = Sha256::new();
                h.update(mo_data);
                h.finalize()
            });

            info_map.insert(
                std::path::PathBuf::from("res_mods")
                    .join("texts")
                    .join("zh_cn")
                    .join("LC_MESSAGES")
                    .join("global.mo")
                    .to_string_lossy()
                    .to_string(),
                serde_json::Value::String(mo_sha256),
            );
        }

        if chat_anticensor {
            let chat_xml = br#"<?xml version="1.0" encoding="utf-8"?>
<dictionary>
	<asia><badWords></badWords></asia>
	<ru><badWords></badWords></ru>
	<cn><badWords></badWords></cn>
	<eu><badWords></badWords></eu>
	<na><badWords></badWords></na>
	<st><badWords></badWords></st>
</dictionary>"#;
            let chat_dest = vf_dir.join("res_mods").join("messenger_oldictionary.xml");
            std::fs::write(&chat_dest, chat_xml)
                .map_err(|e| format!("写出 messenger_oldictionary.xml 失败: {e}"))?;

            let chat_sha256 = hex::encode({
                use sha2::{Digest, Sha256};
                let mut h = Sha256::new();
                h.update(chat_xml);
                h.finalize()
            });

            info_map.insert(
                "res_mods/messenger_oldictionary.xml".to_string(),
                serde_json::Value::String(chat_sha256),
            );
        }

        let deriver_dir = vf_dir.join("derivercrabify");
        std::fs::create_dir_all(&deriver_dir)
            .map_err(|e| format!("创建 derivercrabify 目录失败: {}", e))?;

        let info_path = deriver_dir.join("inst_info.json");
        let info_json = serde_json::to_string_pretty(&info_map)
            .map_err(|e| format!("序列化 inst_info 失败: {}", e))?;
        std::fs::write(&info_path, info_json)
            .map_err(|e| format!("写出 inst_info.json 失败: {}", e))?;

        installations.push(InstallationInfo {
            version_folder: vf.clone(),
            text_installed: text_anticensor,
            chat_installed: chat_anticensor,
        });
    }

    Ok(installations)
}

const METADATA_URL: &str = "https://localizedkorabli.org/metadata/derivercrabify/metadata.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UpdateInfo {
    version: String,
    path: String,
}

#[derive(Debug, Clone, Serialize)]
struct UpdateProgress {
    percent: u64,
}

#[tauri::command]
fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[tauri::command]
async fn check_update() -> Result<Option<UpdateInfo>, String> {
    let resp = reqwest::get(METADATA_URL)
        .await
        .map_err(|e| format!("获取更新信息失败: {e}"))?;
    let info: UpdateInfo = resp
        .json()
        .await
        .map_err(|e| format!("解析更新信息失败: {e}"))?;

    let current = if cfg!(debug_assertions) { "0.0.0" } else { env!("CARGO_PKG_VERSION") };
    if info.version == current {
        Ok(None)
    } else {
        Ok(Some(info))
    }
}

#[tauri::command]
async fn install_update(app: tauri::AppHandle, download_url: String) -> Result<(), String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(&download_url)
        .send()
        .await
        .map_err(|e| format!("下载更新失败: {e}"))?;

    let total = resp.content_length().unwrap_or(0);
    let tmp_path = std::env::temp_dir().join("derivercrabify_update.exe");
    let mut file = std::fs::File::create(&tmp_path)
        .map_err(|e| format!("创建临时文件失败: {e}"))?;

    let mut stream = resp.bytes_stream();
    let mut downloaded: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("下载数据失败: {e}"))?;
        file.write_all(&chunk)
            .map_err(|e| format!("写入文件失败: {e}"))?;
        downloaded += chunk.len() as u64;
        if total > 0 {
            let percent = (downloaded as f64 / total as f64 * 100.0) as u64;
            let _ = app.emit("update-progress", UpdateProgress { percent });
        }
    }

    drop(file);

    std::process::Command::new(&tmp_path)
        .spawn()
        .map_err(|e| format!("启动安装程序失败: {e}"))?;

    app.exit(0);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            scan_instances,
            add_instance,
            refresh_instance,
            install_locale_pack,
            get_app_version,
            check_update,
            install_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
