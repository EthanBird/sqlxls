use crate::engine::Extension;

pub mod read_excel;
pub mod read_dir;
pub mod read_clipboard;
pub mod mock_data;
pub mod read_json;  // 新增
pub mod read_api;   // 新增

pub fn register_all() -> Vec<Box<dyn Extension>> {
    vec![
        Box::new(read_excel::ReadExcelExt),
        Box::new(read_dir::ReadDirExt),
        Box::new(read_clipboard::ReadClipboardExt),
        Box::new(mock_data::MockDataExt),
        Box::new(read_json::ReadJsonExt), // 注册 readjson
        Box::new(read_api::ReadApiExt),   // 注册 readapi
    ]
}