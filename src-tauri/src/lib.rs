mod instances;
mod mo;

use instances::{InstanceInfo, InstallationInfo};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::PathBuf;
use tauri::Emitter;

const LOCALE_URL: &str = "https://dl.localizedkorabli.org/i18n/zh_cn360/global.mo";

#[derive(Clone, Serialize)]
struct ProgressPayload {
    message: String,
    downloaded: u64,
    total: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheInfo {
    etag: String,
}

fn emit_progress(app: &tauri::AppHandle, message: &str, downloaded: u64, total: u64) {
    let _ = app.emit("locale-progress", ProgressPayload {
        message: message.to_string(),
        downloaded,
        total,
    });
}

fn cache_dir() -> Result<PathBuf, String> {
    let dir = dirs::data_dir()
        .ok_or_else(|| "找不到用户数据目录".to_string())?
        .join("wowscn_derivercrabify");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("创建缓存目录失败: {}", e))?;
    Ok(dir)
}

fn load_cache_etag() -> Option<String> {
    let cache_file = cache_dir().ok()?.join("cache_info.json");
    let content = std::fs::read_to_string(&cache_file).ok()?;
    let info: CacheInfo = serde_json::from_str(&content).ok()?;
    Some(info.etag)
}

fn save_cache(etag: &str, data: &[u8]) -> Result<(), String> {
    let dir = cache_dir()?;

    let info = CacheInfo {
        etag: etag.to_string(),
    };
    let json = serde_json::to_string(&info)
        .map_err(|e| format!("序列化缓存信息失败: {}", e))?;
    std::fs::write(dir.join("cache_info.json"), json)
        .map_err(|e| format!("写入缓存信息失败: {}", e))?;

    std::fs::write(dir.join("global.mo"), data)
        .map_err(|e| format!("写入缓存文件失败: {}", e))?;

    Ok(())
}

fn download_mo(app: &tauri::AppHandle) -> Result<Vec<u8>, String> {
    if let Some(etag) = load_cache_etag() {
        let cache_path = cache_dir()?.join("global.mo");
        if cache_path.is_file() {
            emit_progress(app, "正在连接服务器...", 0, 0);

            let client = reqwest::blocking::Client::new();
            let resp = client
                .get(LOCALE_URL)
                .header("If-None-Match", &etag)
                .send()
                .map_err(|e| format!("下载语言包失败: {}", e))?;

            if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
                emit_progress(app, "语言包未变化，使用缓存", 0, 0);
                return std::fs::read(&cache_path)
                    .map_err(|e| format!("读取缓存文件失败: {}", e));
            }

            let new_total = resp.content_length().unwrap_or(0);
            let new_etag = resp
                .headers()
                .get("etag")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());

            emit_progress(app, "正在下载语言包...", 0, new_total);

            let mut response = resp;
            let mut buf = Vec::new();
            if new_total > 0 {
                buf.reserve(new_total as usize);
            }
            let mut downloaded: u64 = 0;
            let mut last_emit: u64 = 0;

            loop {
                let mut chunk = vec![0u8; 65536];
                let n = response
                    .read(&mut chunk)
                    .map_err(|e| format!("读取下载数据失败: {}", e))?;
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                downloaded += n as u64;

                let threshold = if new_total > 0 { new_total.max(1) / 50 } else { 256 * 1024 };
                if downloaded - last_emit >= threshold || downloaded == new_total {
                    last_emit = downloaded;
                    emit_progress(app, "正在下载语言包...", downloaded, new_total);
                }
            }

            if let Some(etag_val) = new_etag {
                save_cache(&etag_val, &buf)?;
            }

            return Ok(buf);
        }
    }

    emit_progress(app, "正在连接服务器...", 0, 0);

    let client = reqwest::blocking::Client::new();
    let mut response = client
        .get(LOCALE_URL)
        .send()
        .map_err(|e| format!("下载语言包失败: {}", e))?;

    let total = response.content_length().unwrap_or(0);
    let new_etag = response
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    emit_progress(app, "正在下载语言包...", 0, total);

    let mut buf = Vec::new();
    if total > 0 {
        buf.reserve(total as usize);
    }
    let mut downloaded: u64 = 0;
    let mut last_emit: u64 = 0;

    loop {
        let mut chunk = vec![0u8; 65536];
        let n = response
            .read(&mut chunk)
            .map_err(|e| format!("读取下载数据失败: {}", e))?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        downloaded += n as u64;

        let threshold = if total > 0 { total.max(1) / 50 } else { 256 * 1024 };
        if downloaded - last_emit >= threshold || downloaded == total {
            last_emit = downloaded;
            emit_progress(app, "正在下载语言包...", downloaded, total);
        }
    }

    if let Some(etag_val) = new_etag {
        save_cache(&etag_val, &buf)?;
    }

    Ok(buf)
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
        .map(|vf| InstallationInfo {
            installed: instances::check_installed(p, vf),
            version_folder: vf.clone(),
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
        .map(|vf| InstallationInfo {
            installed: instances::check_installed(p, vf),
            version_folder: vf.clone(),
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
) -> Result<Vec<InstallationInfo>, String> {
    let p = std::path::Path::new(&instance_path);

    instances::is_valid_instance(p)
        .map_err(|e| format!("非法的 CN360 实例路径: {}", e))?;

    emit_progress(&app, "正在连接服务器...", 0, 0);

    let mo_data = download_mo(&app)?;

    emit_progress(&app, "正在处理语言文件...", 0, 0);

    let mo_file = mo::MoFile::parse(&mo_data)
        .map_err(|e| format!("解析 global.mo 失败: {}", e))?;

    let filtered = mo_file.filter_eventum();
    let new_mo_data = filtered.to_bytes()
        .map_err(|e| format!("生成 global.mo 失败: {}", e))?;

    let version_folders = instances::find_version_folders(p);

    let mut installations = Vec::new();

    for vf in &version_folders {
        emit_progress(&app, &format!("正在安装到版本 {} ...", vf), 0, 0);

        let vf_dir = p.join("bin").join(vf);
        let target_dir = vf_dir
            .join("res_mods")
            .join("texts")
            .join("zh_cn")
            .join("LC_MESSAGES");
        std::fs::create_dir_all(&target_dir)
            .map_err(|e| format!("创建目录失败: {}", e))?;

        let mo_dest = target_dir.join("global.mo");
        std::fs::write(&mo_dest, &new_mo_data)
            .map_err(|e| format!("写出 global.mo 失败: {}", e))?;

        let installed_sha256 = hex::encode({
            use sha2::{Digest, Sha256};
            let mut h = Sha256::new();
            h.update(&new_mo_data);
            h.finalize()
        });

        let relative_path = std::path::PathBuf::from("res_mods")
            .join("texts")
            .join("zh_cn")
            .join("LC_MESSAGES")
            .join("global.mo");
        let relative_path_str = relative_path.to_string_lossy().to_string();

        let mut info_map = serde_json::Map::new();
        info_map.insert(
            relative_path_str,
            serde_json::Value::String(installed_sha256),
        );

        let deriver_dir = vf_dir.join("derivercrabify");
        std::fs::create_dir_all(&deriver_dir)
            .map_err(|e| format!("创建 derivercrabify 目录失败: {}", e))?;

        let info_path = deriver_dir.join("inst_info.json");
        let info_json = serde_json::to_string_pretty(&info_map)
            .map_err(|e| format!("序列化 inst_info 失败: {}", e))?;
        std::fs::write(&info_path, info_json)
            .map_err(|e| format!("写出 inst_info.json 失败: {}", e))?;

        installations.push(InstallationInfo {
            installed: true,
            version_folder: vf.clone(),
        });
    }

    Ok(installations)
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
