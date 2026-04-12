use crate::engine::Extension;

pub mod read_excel;
pub mod read_dir;
pub mod read_clipboard;
pub mod mock_data;

pub fn register_all() -> Vec<Box<dyn Extension>> {
    vec![
        Box::new(read_excel::ReadExcelExt), // 注册 readexcel 功能
        Box::new(read_dir::ReadDirExt),     // 注册 readdir 功能
        // 未来新功能如 readmysql, readapi 都可以写在这里
        Box::new(read_clipboard::ReadClipboardExt), // 新增
        Box::new(mock_data::MockDataExt),           // 新增
    ]
}