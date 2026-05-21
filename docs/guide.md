# 介绍

你需要实现一个Windows 64位桌面应用。这个应用能自动检测安装在设备上的CN360服务器战舰世界客户端，并将ASIA服务器战舰世界客户端的语言文件作为mod安装到360端。

## 1. 定位360端

你需要通过注册表寻找360 Wargaming Game Center的目录，并在目录的preferences.xml中找到CN360端的安装目录。

参考这段Python代码：
```
def _find_from_registry() -> List[Tuple[str, str]]:
    log("Scanning LGC registries...")
    found_list: List[Tuple[str, str]] = []
    seen_paths: Set[str] = set()
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, r'Software\Classes\wgc360\DefaultIcon') as key:
            lgc_dir_str, _ = winreg.QueryValueEx(key, '')

        if ',' in lgc_dir_str:
            lgc_dir_str = lgc_dir_str.split(',')[0]

        preferences_path = Path(lgc_dir_str).parent / 'preferences.xml'
        if not preferences_path.is_file():
            return found_list

        pref_root = Et.parse(preferences_path).getroot()
        games_block = pref_root.find('.//application/games_manager/games')
        if games_block is None:
            return found_list

        for game in games_block.findall('.//game'):
            wd_elem = game.find('working_dir')
            if wd_elem is not None:
                path_str = wd_elem.text
                path = Path(path_str)
                type_code = is_valid_instance(path)
                normalized_path = os.path.normpath(path_str)

                if type_code and normalized_path not in seen_paths:
                    found_list.append((normalized_path, type_code))
                    seen_paths.add(normalized_path)
                    log(f"Mir Korabley instance found in LGC registries: {normalized_path}")

    except FileNotFoundError:
        log("LGC registry key or preferences.xml not found. Skipping registry scan.")
    except Exception as e:
        log(f"Error scanning registry: {e}")
    return found_list

def is_valid_instance(path: Path) -> bool:
    try:
        if not path.is_dir():
            return False
        if not (path / 'wgc360_api.exe').is_file():
            return False
        if not (path / 'bin').is_dir():
            return False
    except Exception:
        return False

    xml_path = path / 'game_info.xml'
    if xml_path.is_file():
        try:
            tree = Et.parse(xml_path)
            game_id_elem = tree.find('.//game/id')
            if game_id_elem is not None:
                game_id = game_id_elem.text
                if game_id == 'WOWS.CN.PRODUCTION':
                    return True
        except Exception as e:
            log(f"Error parsing game_info.xml at {path}: {e}")

    return False
```

同时也允许用户手动添加未被扫描出来的cn360服实例，但必须不与列表中已被添加的任何实例有相同路径，且必须通过is_valid_instance的检查。

存在多个实例时，要求用户选择其中的一个，以便执行后续操作。

对于用户选择的实例，进入其安装目录。再进入其bin目录下。对于bin目录下所有以纯数字命名的文件夹，以数字数值从大到小的顺序执行以下检查：数字文件夹中是否存在bin64/WorldOfWarships64.exe。当最多2个数字文件夹通过检查后直接停止检查，保留这最多2个数字文件夹名称。

## 2. 下载并处理亚服语言文件

### 2.1 获取更新信息

访问[更新链接](https://wgus-eu.wargaming.net/api/v1/patches_chain/?game_id=WOWS.WW.PRODUCTION&protocol_version=1.11&metadata_version=20251121135024&metadata_protocol_version=7.10&client_type=high&lang=ZH_SG&chain_id=f21&game_installation=false&gc_publisher=wargaming&client_current_version=0&hotfix_current_version=0&locale_current_version=0&sdcontent_current_version=0)以获取更新信息xml。

xml中patches_chain列表包含多个patch元素，找到patch元素中files元素（该元素下仅包含一个file元素）中**name字段含locale的**file元素。再到该file元素所在的files元素中的torrent元素的urls下找到包含本地化包更新的torrent，记录一下name字段，以便之后提供缓存。

### 2.2 下载语言文件

在该torrent中，下载文件名包含locale的那个.dspkg文件。此文件为压缩包格式，在其中寻找*/texts/zh_sg/LC_MESSAGES/global.mo（应有1~2个文件匹配，当有2个文件匹配时，比较其在压缩包中的路径bin/[number]/texts/zh_sg/LC_MESSAGES/global.mo，取number数值更大者）并解压，作为语言文件基础。

### 2.3 修改语言文件

使用支持修改Gettext MO的库打开global.mo，将其所有MO Entry（除了msgid中含有“EVENTUM”的Entry外）复制到新的MO实例，然后导出为新的global.mo。此步骤结束后建立缓存——之前2.1节记录的name字段以及导出的global.mo的文件位置和sha256值。若下次执行第1.节获取更新信息获得了同样的含locale的name字段，且文件SHA256无误，则直接跳到第3.节。

## 3. 将语言文件安装到360端

对于第1.节中保留的数字文件夹执行以下操作：将global.mo复制到数字文件夹/res_mods/texts/zh_cn/LC_MESSAGES/下。并分别在数字文件夹/derivercrabify/下创建inst_info.json，结构为一个字典，包含一个entry：键名为被复制的global.mo相对于数字文件夹的路径，值为该文件的SHA256。第1.节用户选择目录时，则可进行检查，如.json存在，键名存在，且键名目录拼接后找到的文件存在并匹配SHA256值，则将该实例的该数字文件夹标记为已安装，否则为未安装。