use quick_xml::de::from_str;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename = "root")]
pub struct PreferencesRoot {
    #[serde(rename = "application")]
    pub application: Option<PreferencesApplication>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PreferencesApplication {
    #[serde(rename = "games_manager")]
    pub games_manager: Option<GamesManager>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GamesManager {
    #[serde(rename = "games")]
    pub games: Option<GamesBlock>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GamesBlock {
    #[serde(rename = "game", default)]
    pub game: Vec<GameEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GameEntry {
    #[serde(rename = "working_dir", default)]
    pub working_dir: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename = "root")]
pub struct GameInfoRoot {
    #[serde(rename = "game", default)]
    pub game: Vec<GameInfoGame>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GameInfoGame {
    #[serde(rename = "id", default)]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceInfo {
    pub path: String,
    pub version_folders: Vec<String>,
    pub installations: Vec<InstallationInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallationInfo {
    pub version_folder: String,
    pub installed: bool,
}

fn instances_file() -> PathBuf {
    let dir = dirs::data_dir()
        .expect("找不到用户数据目录")
        .join("wowscn_derivercrabify");
    std::fs::create_dir_all(&dir).ok();
    dir.join("instances.json")
}

fn load_persisted_instances() -> Vec<String> {
    let path = instances_file();
    if !path.is_file() {
        return Vec::new();
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let paths: Vec<String> = match serde_json::from_str(&content) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    paths
}

fn save_persisted_instances(paths: &[String]) {
    let path = instances_file();
    let json = match serde_json::to_string(paths) {
        Ok(j) => j,
        Err(_) => return,
    };
    std::fs::write(&path, json).ok();
}

pub fn scan_instances() -> Result<Vec<InstanceInfo>, String> {
    let mut found = Vec::new();
    let mut seen = HashSet::new();

    if let Ok(reg_key) = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Classes\wgc360\DefaultIcon")
    {
        if let Ok(lgc_dir_str) = reg_key.get_value::<String, &str>("") {
            let lgc_dir_str = if lgc_dir_str.contains(',') {
                lgc_dir_str.split(',').next().unwrap().to_string()
            } else {
                lgc_dir_str
            };

            let lgc_path = PathBuf::from(&lgc_dir_str);
            if let Some(ref path) = lgc_path.parent().map(|p| p.join("preferences.xml")) {
                if path.is_file() {
                    if let Ok(xml_content) = std::fs::read_to_string(path) {
                        if let Ok(prefs) = from_str::<PreferencesRoot>(&xml_content) {
                            if let Some(app) = &prefs.application {
                                if let Some(gm) = &app.games_manager {
                                    if let Some(games) = &gm.games {
                                        for game in &games.game {
                                            if let Some(wd) = &game.working_dir {
                                                let p = Path::new(wd);
                                                let normalized = normalize_path(wd);
                                                if !seen.contains(&normalized) {
                                                    seen.insert(normalized.clone());
                                                    if is_valid_instance(p).is_ok() {
                                                        found.push(build_instance_info(p, &normalized));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let persisted = load_persisted_instances();
    for p_str in &persisted {
        let p = Path::new(p_str);
        let normalized = normalize_path(p_str);
        if !seen.contains(&normalized) {
            seen.insert(normalized.clone());
            if is_valid_instance(p).is_ok() {
                found.push(build_instance_info(p, &normalized));
            }
        }
    }

    if found.is_empty() {
        return Err("未找到 CN360 战舰世界实例".to_string());
    }

    Ok(found)
}

fn build_instance_info(path: &Path, normalized: &str) -> InstanceInfo {
    let version_folders = find_version_folders(path);
    let installations: Vec<InstallationInfo> = version_folders
        .iter()
        .map(|vf| InstallationInfo {
            installed: check_installed(path, vf),
            version_folder: vf.clone(),
        })
        .collect();

    InstanceInfo {
        path: normalized.to_string(),
        version_folders,
        installations,
    }
}

pub fn is_valid_instance(path: &Path) -> Result<(), String> {
    if !path.is_dir() {
        return Err("路径不是有效的目录".to_string());
    }
    let api_exe = path.join("wgc360_api.exe");
    if !api_exe.is_file() {
        return Err(format!("找不到 {}", api_exe.display()));
    }
    let bin_dir = path.join("bin");
    if !bin_dir.is_dir() {
        return Err(format!("找不到 {} 目录", bin_dir.display()));
    }

    let xml_path = path.join("game_info.xml");
    if !xml_path.is_file() {
        return Err(format!("找不到 {}", xml_path.display()));
    }
    let content = std::fs::read_to_string(&xml_path)
        .map_err(|e| format!("读取 {} 失败: {}", xml_path.display(), e))?;
    let info: GameInfoRoot = from_str(&content)
        .map_err(|e| format!("解析 {} 失败: {}", xml_path.display(), e))?;
    for g in &info.game {
        if let Some(id) = &g.id {
            if id == "WOWS.CN.PRODUCTION" {
                return Ok(());
            }
        }
    }

    Err(format!("{} 中未找到 WOWS.CN.PRODUCTION", xml_path.display()))
}

pub fn find_version_folders(path: &Path) -> Vec<String> {
    let bin_dir = path.join("bin");
    if !bin_dir.is_dir() {
        return Vec::new();
    }

    let mut numeric_dirs: Vec<u64> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&bin_dir) {
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();
            if let Ok(num) = name.parse::<u64>() {
                let exe_path = entry.path().join("bin64").join("WorldOfWarships64.exe");
                if exe_path.is_file() {
                    numeric_dirs.push(num);
                }
            }
        }
    }

    numeric_dirs.sort_by(|a, b| b.cmp(a));
    numeric_dirs.truncate(2);
    numeric_dirs.iter().map(|n| n.to_string()).collect()
}

pub fn check_installed(instance_path: &Path, version_folder: &str) -> bool {
    let info_path = instance_path
        .join("bin")
        .join(version_folder)
        .join("derivercrabify")
        .join("inst_info.json");

    if !info_path.is_file() {
        return false;
    }

    let content = match std::fs::read_to_string(&info_path) {
        Ok(c) => c,
        Err(_) => return false,
    };

    let info: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return false,
    };

    if let Some(obj) = info.as_object() {
        for (relative_path, expected_sha256) in obj {
            let file_path = instance_path
                .join("bin")
                .join(version_folder)
                .join(relative_path);

            if !file_path.is_file() {
                return false;
            }

            let actual_sha256 = match sha256_file(&file_path) {
                Ok(h) => h,
                Err(_) => return false,
            };

            if actual_sha256 != expected_sha256.as_str().unwrap_or("") {
                return false;
            }
        }
        return true;
    }

    false
}

pub fn persist_instance_path(path: &str) -> Result<(), String> {
    let mut paths = load_persisted_instances();
    let normalized = normalize_path(path);
    if !paths.contains(&normalized) {
        paths.push(normalized);
        save_persisted_instances(&paths);
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let data =
        std::fs::read(path).map_err(|e| format!("读取文件计算 SHA256 失败: {}", e))?;
    let mut hasher = Sha256::new();
    hasher.update(&data);
    Ok(hex::encode(hasher.finalize()))
}

fn normalize_path(path: &str) -> String {
    use std::path::Component;

    let mut result = String::new();

    for component in Path::new(path).components() {
        match component {
            Component::Prefix(_) => {
                result.push_str(&component.as_os_str().to_string_lossy());
            }
            Component::RootDir => {
                result.push_str(&component.as_os_str().to_string_lossy());
            }
            Component::Normal(_) => {
                if !result.is_empty() && !result.ends_with('\\') {
                    result.push('\\');
                }
                result.push_str(&component.as_os_str().to_string_lossy());
            }
            _ => {}
        }
    }

    result
}
