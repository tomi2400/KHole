#[tauri::command]
fn fixtures() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir("/Users/tamas/Documents/Claude/VS_Code/KHole/spike/fixtures").unwrap()
        .filter_map(|e| e.ok()).map(|e| e.path().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

#[tauri::command]
fn report(line: String) { println!("SPIKE {line}"); }

#[tauri::command]
fn done(app: tauri::AppHandle) { app.exit(0); }

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![fixtures, report, done])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
