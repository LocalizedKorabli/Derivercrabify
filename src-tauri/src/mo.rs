use polib::catalog::Catalog;
use polib::message::Message;
use polib::metadata::CatalogMetadata;
use polib::mo_file;
use std::collections::BTreeMap;

const MO_MAGIC: u32 = 0x950412de;

pub struct MoFile {
    pub entries: BTreeMap<Vec<u8>, Vec<u8>>,
}

impl MoFile {
    pub fn parse(data: &[u8]) -> Result<Self, String> {
        if data.len() < 28 {
            return Err("MO file too short for header".to_string());
        }

        let magic = read_u32_le(data, 0);
        if magic != MO_MAGIC {
            return Err(format!("Invalid MO magic number: 0x{:08x}", magic));
        }

        let num_strings = read_u32_le(data, 8) as usize;
        let orig_table_offset = read_u32_le(data, 12) as usize;
        let trans_table_offset = read_u32_le(data, 16) as usize;

        if num_strings == 0 {
            return Ok(MoFile {
                entries: BTreeMap::new(),
            });
        }

        let mut entries = BTreeMap::new();

        for i in 0..num_strings {
            let orig_len = read_u32_le(data, orig_table_offset + i * 8) as usize;
            let orig_offset = read_u32_le(data, orig_table_offset + i * 8 + 4) as usize;

            let trans_len = read_u32_le(data, trans_table_offset + i * 8) as usize;
            let trans_offset = read_u32_le(data, trans_table_offset + i * 8 + 4) as usize;

            let msgid = read_bytes(data, orig_offset, orig_len)?;
            let msgstr = read_bytes(data, trans_offset, trans_len)?;

            entries.insert(msgid, msgstr);
        }

        Ok(MoFile { entries })
    }

    pub fn filter_eventum(&self) -> MoFile {
        let mut new_entries = BTreeMap::new();

        for (msgid, msgstr) in &self.entries {
            let msgid_str = String::from_utf8_lossy(msgid);
            if !msgid_str.contains("EVENTUM") {
                new_entries.insert(msgid.clone(), msgstr.clone());
            }
        }

        MoFile {
            entries: new_entries,
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        let mut metadata = CatalogMetadata::new();
        let mut catalog = Catalog::new(metadata.clone());

        for (msgid, msgstr) in &self.entries {
            let msgid_full = bytes_to_nul_split(msgid);
            if msgid_full.len() == 1 && msgid_full[0].is_empty() {
                let msgstr_full = bytes_to_nul_split(msgstr);
                let header_text = msgstr_full.first().map(|s| s.as_str()).unwrap_or("");
                if !header_text.is_empty() {
                    metadata = CatalogMetadata::parse(header_text)
                        .map_err(|e| format!("解析头部元数据失败: {}", e))?;
                }
                continue;
            }

            let message = if msgid_full.len() == 1 {
                let msgstr_full = bytes_to_nul_split(msgstr);
                let msgstr_single = msgstr_full.first().map(|s| s.as_str()).unwrap_or("");

                let mut builder = Message::build_singular();
                builder.with_msgid(msgid_full[0].clone());
                builder.with_msgstr(msgstr_single.to_string());
                builder.done()
            } else {
                let msgstr_parts = bytes_to_nul_split(msgstr);

                let mut builder = Message::build_plural();
                builder.with_msgid(msgid_full[0].clone());
                builder.with_msgid_plural(msgid_full[1].clone());
                builder.with_msgstr_plural(msgstr_parts);
                builder.done()
            };

            catalog.append_or_update(message);
        }

        catalog.metadata = metadata;

        let dir = tempfile::tempdir().map_err(|e| format!("创建临时目录失败: {}", e))?;
        let mo_path = dir.path().join("output.mo");

        mo_file::write(&catalog, &mo_path)
            .map_err(|e| format!("写出 MO 文件失败: {}", e))?;

        let mo_data = std::fs::read(&mo_path)
            .map_err(|e| format!("读取临时 MO 文件失败: {}", e))?;

        Ok(mo_data)
    }
}

fn bytes_to_nul_split(data: &[u8]) -> Vec<String> {
    if data.is_empty() {
        return vec![String::new()];
    }

    let mut parts = Vec::new();
    let mut start = 0;
    let end = if data.last() == Some(&0) {
        data.len() - 1
    } else {
        data.len()
    };

    for i in 0..end {
        if data[i] == 0 {
            parts.push(String::from_utf8_lossy(&data[start..i]).to_string());
            start = i + 1;
        }
    }
    parts.push(String::from_utf8_lossy(&data[start..end]).to_string());

    parts
}

fn read_u32_le(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

fn read_bytes(data: &[u8], offset: usize, length: usize) -> Result<Vec<u8>, String> {
    if offset + length > data.len() {
        return Err(format!(
            "String data out of bounds: offset={}, length={}, data_len={}",
            offset,
            length,
            data.len()
        ));
    }
    Ok(data[offset..offset + length].to_vec())
}
