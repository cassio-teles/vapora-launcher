#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use futures_util::StreamExt;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use std::{path::{Path, PathBuf}, process::Command, net::TcpListener, time::{SystemTime, UNIX_EPOCH}};
use tauri::{AppHandle, Emitter, Manager};
use tokio::{io::AsyncWriteExt, process::Command as TokioCommand, time::{sleep, Duration}};

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Game {
    id: i64,
    title: String,
    developer: String,
    description: String,
    install_path: Option<String>,
    executable: Option<String>,
    cover: Option<String>,
    source: String,
    added_at: String,
    is_catalog: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Repository {
    id: i64,
    name: String,
    url: String,
    enabled: bool,
    last_sync: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct DownloadSource {
    id: i64,
    game_id: i64,
    repository_id: i64,
    repository_name: String,
    name: String,
    url: String,
    kind: String,
    size: Option<String>,
    sha256: Option<String>,
}

#[derive(Debug, Serialize)]
struct StoreFeaturedGame { appid: u64, name: String, header_image: String, discount_percent: u64, final_price: u64, currency: String }

#[tauri::command]
async fn fetch_store_featured() -> Result<Vec<StoreFeaturedGame>, String> {
    let client = reqwest::Client::builder().user_agent("Vapora/0.7 desktop storefront").build().map_err(|e| e.to_string())?;
    let response = client.get("https://store.steampowered.com/api/featuredcategories/?cc=br&l=portuguese").send().await.map_err(|e| format!("Não foi possível consultar a loja Steam: {e}"))?.error_for_status().map_err(|e| format!("A loja Steam respondeu com erro: {e}"))?;
    let data: serde_json::Value = response.json().await.map_err(|e| format!("Resposta da loja Steam inválida: {e}"))?;
    let currency = data.get("currency").and_then(|v| v.as_str()).unwrap_or("BRL").to_string();
    let mut games = Vec::new(); let mut seen = std::collections::HashSet::new();
    for category in ["specials", "top_sellers", "new_releases", "coming_soon"] {
        if let Some(items) = data.get(category).and_then(|v| v.get("items")).and_then(|v| v.as_array()) {
            for item in items {
                let Some(appid) = item.get("id").and_then(|v| v.as_u64()) else { continue; };
                let Some(name) = item.get("name").and_then(|v| v.as_str()) else { continue; };
                let image = item.get("header_image").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if image.is_empty() || !seen.insert(appid) { continue; }
                games.push(StoreFeaturedGame { appid, name: name.to_string(), header_image: image, discount_percent: item.get("discount_percent").and_then(|v| v.as_u64()).unwrap_or(0), final_price: item.get("final_price").and_then(|v| v.as_u64()).unwrap_or(0), currency: currency.clone() });
                if games.len() >= 24 { break; }
            }
        }
        if games.len() >= 24 { break; }
    }
    if games.is_empty() { return Err("A Steam não retornou jogos em destaque. Tente novamente mais tarde.".into()); }
    Ok(games)
}

#[tauri::command]
async fn fetch_steam_game_details(appid: u64) -> Result<serde_json::Value, String> {
    let url = format!("https://store.steampowered.com/api/appdetails?appids={appid}&cc=br&l=brazilian");
    let client = reqwest::Client::builder().user_agent("Vapora/0.8 desktop storefront").build().map_err(|e| e.to_string())?;
    let response = client.get(url).send().await.map_err(|e| format!("Falha ao consultar detalhes do jogo: {e}"))?.error_for_status().map_err(|e| e.to_string())?;
    let payload: serde_json::Value = response.json().await.map_err(|e| format!("Resposta de detalhes inválida: {e}"))?;
    let key = appid.to_string();
    let entry = payload.get(&key).ok_or("A Steam não retornou este jogo")?;
    if entry.get("success").and_then(|v| v.as_bool()) != Some(true) { return Err("Detalhes indisponíveis para este jogo na Steam".into()); }
    entry.get("data").cloned().ok_or_else(|| "A Steam retornou detalhes vazios".into())
}

#[tauri::command]
async fn fetch_steam_game_reviews(appid: u64) -> Result<Vec<serde_json::Value>, String> {
    let url = format!("https://store.steampowered.com/appreviews/{appid}?json=1&language=all&num_per_page=10&purchase_type=all");
    let client = reqwest::Client::builder().user_agent("Vapora/0.8 desktop storefront").build().map_err(|e| e.to_string())?;
    let response = client.get(url).send().await.map_err(|e| format!("Falha ao consultar avaliações: {e}"))?.error_for_status().map_err(|e| e.to_string())?;
    let payload: serde_json::Value = response.json().await.map_err(|e| format!("Resposta de avaliações inválida: {e}"))?;
    Ok(payload.get("reviews").and_then(|v| v.as_array()).cloned().unwrap_or_default())
}

#[derive(Debug, Deserialize)]
struct Catalog { games: Vec<CatalogGame> }

#[derive(Debug, Deserialize)]
struct CatalogGame {
    title: String,
    #[serde(default)] developer: String,
    #[serde(default)] description: String,
    #[serde(default)] cover: Option<String>,
    #[serde(default)] sources: Vec<CatalogSource>,
}

#[derive(Debug, Deserialize)]
struct CatalogSource {
    name: String,
    url: String,
    #[serde(default = "default_source_kind")] kind: String,
    #[serde(default)] size: Option<String>,
    #[serde(default)] sha256: Option<String>,
}

fn default_source_kind() -> String { "direct".into() }

#[derive(Clone, Serialize)]
struct DownloadEvent {
    source_id: i64,
    game_id: i64,
    game_title: String,
    file_name: String,
    downloaded: u64,
    total: Option<u64>,
    status: String,
    path: Option<String>,
    error: Option<String>,
}

fn db_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("vapora.sqlite3"))
}

fn connect(app: &AppHandle) -> Result<Connection, String> {
    let db = Connection::open(db_path(app)?).map_err(|e| e.to_string())?;
    db.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA foreign_keys=ON;
         CREATE TABLE IF NOT EXISTS games(
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           title TEXT NOT NULL,
           developer TEXT NOT NULL DEFAULT '',
           description TEXT NOT NULL DEFAULT '',
           install_path TEXT,
           executable TEXT,
           cover TEXT,
           source TEXT NOT NULL DEFAULT 'Local',
           added_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
           is_catalog INTEGER NOT NULL DEFAULT 0,
           repository_id INTEGER
         );
         CREATE TABLE IF NOT EXISTS repositories(
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           name TEXT NOT NULL,
           url TEXT NOT NULL UNIQUE,
           enabled INTEGER NOT NULL DEFAULT 1,
           last_sync TEXT
         );
         CREATE TABLE IF NOT EXISTS download_sources(
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           game_id INTEGER NOT NULL,
           repository_id INTEGER NOT NULL,
           name TEXT NOT NULL,
           url TEXT NOT NULL,
           kind TEXT NOT NULL DEFAULT 'direct',
           size TEXT,
           sha256 TEXT,
           FOREIGN KEY(game_id) REFERENCES games(id) ON DELETE CASCADE,
           FOREIGN KEY(repository_id) REFERENCES repositories(id) ON DELETE CASCADE
         );"
    ).map_err(|e| e.to_string())?;
    Ok(db)
}

fn game_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Game> {
    Ok(Game {
        id: r.get(0)?, title: r.get(1)?, developer: r.get(2)?, description: r.get(3)?,
        install_path: r.get(4)?, executable: r.get(5)?, cover: r.get(6)?, source: r.get(7)?,
        added_at: r.get(8)?, is_catalog: r.get::<_, i64>(9)? != 0,
    })
}

#[tauri::command]
fn list_games(app: AppHandle) -> Result<Vec<Game>, String> {
    let db = connect(&app)?;
    let mut s = db.prepare("SELECT id,title,developer,description,install_path,executable,cover,source,added_at,is_catalog FROM games WHERE is_catalog=0 OR executable IS NOT NULL ORDER BY title COLLATE NOCASE").map_err(|e| e.to_string())?;
    let rows = s.query_map([], game_from_row).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
fn list_catalog_games(app: AppHandle) -> Result<Vec<Game>, String> {
    let db = connect(&app)?;
    let mut s = db.prepare("SELECT id,title,developer,description,install_path,executable,cover,source,added_at,is_catalog FROM games WHERE is_catalog=1 ORDER BY title COLLATE NOCASE").map_err(|e| e.to_string())?;
    let rows = s.query_map([], game_from_row).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
fn add_game(app: AppHandle, title: String, developer: String, description: String, install_path: Option<String>, executable: Option<String>, cover: Option<String>) -> Result<i64, String> {
    let db = connect(&app)?;
    db.execute("INSERT INTO games(title,developer,description,install_path,executable,cover,is_catalog) VALUES(?1,?2,?3,?4,?5,?6,0)", params![title,developer,description,install_path,executable,cover]).map_err(|e| e.to_string())?;
    Ok(db.last_insert_rowid())
}

#[tauri::command]
fn remove_game(app: AppHandle, id: i64) -> Result<(), String> {
    connect(&app)?.execute("DELETE FROM games WHERE id=?1", [id]).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn list_repositories(app: AppHandle) -> Result<Vec<Repository>, String> {
    let db = connect(&app)?;
    let mut s = db.prepare("SELECT id,name,url,enabled,last_sync FROM repositories ORDER BY name").map_err(|e| e.to_string())?;
    let rows = s.query_map([], |r| Ok(Repository { id:r.get(0)?, name:r.get(1)?, url:r.get(2)?, enabled:r.get::<_,i64>(3)? != 0, last_sync:r.get(4)? })).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
fn add_repository(app: AppHandle, name: String, url: String) -> Result<i64, String> {
    let parsed = reqwest::Url::parse(&url).map_err(|_| "URL inválida".to_string())?;
    if parsed.scheme() != "https" { return Err("Por segurança, os repositórios devem usar HTTPS".into()); }
    if name.trim().is_empty() { return Err("Informe o nome do repositório".into()); }
    let db = connect(&app)?;
    db.execute("INSERT INTO repositories(name,url) VALUES(?1,?2)", params![name.trim(),url]).map_err(|e| e.to_string())?;
    Ok(db.last_insert_rowid())
}

#[tauri::command]
async fn sync_repository(app: AppHandle, id: i64) -> Result<usize, String> {
    let (url, repo_name) = {
        let db = connect(&app)?;
        db.query_row("SELECT url,name FROM repositories WHERE id=?1 AND enabled=1", [id], |r| Ok((r.get::<_,String>(0)?, r.get::<_,String>(1)?))).map_err(|e| e.to_string())?
    };
    let response = reqwest::Client::builder().user_agent("Vapora/0.9 (catalog client)").build().map_err(|e| e.to_string())?.get(&url).send().await.map_err(|e| format!("Falha de conexão com o catálogo: {e}"))?;
    if response.status() == reqwest::StatusCode::FORBIDDEN { return Err("A fonte recusou o acesso (HTTP 403 Forbidden). O servidor não autorizou esta requisição; o Vapora não consegue importar esse catálogo enquanto o provedor mantiver o bloqueio.".into()); }
    let response = response.error_for_status().map_err(|e| format!("O catálogo respondeu com erro HTTP: {e}"))?;
    if response.url().scheme() != "https" { return Err("O repositório redirecionou para uma conexão não segura".into()); }
    let catalog: Catalog = response.json().await.map_err(|e| format!("Catálogo JSON inválido: {e}"))?;
    let db = connect(&app)?;
    let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
    let mut count = 0usize;
    for game in catalog.games {
        let existing: Option<i64> = tx.query_row("SELECT id FROM games WHERE repository_id=?1 AND title=?2", params![id,game.title], |r| r.get(0)).ok();
        let game_id = if let Some(existing_id) = existing {
            tx.execute("UPDATE games SET developer=?1,description=?2,cover=?3,source=?4 WHERE id=?5", params![game.developer,game.description,game.cover,repo_name,existing_id]).map_err(|e| e.to_string())?;
            tx.execute("DELETE FROM download_sources WHERE game_id=?1", [existing_id]).map_err(|e| e.to_string())?;
            existing_id
        } else {
            tx.execute("INSERT INTO games(title,developer,description,cover,source,is_catalog,repository_id) VALUES(?1,?2,?3,?4,?5,1,?6)", params![game.title,game.developer,game.description,game.cover,repo_name,id]).map_err(|e| e.to_string())?;
            tx.last_insert_rowid()
        };
        for source in game.sources {
            let parsed = if source.kind == "magnet" { None } else { Some(reqwest::Url::parse(&source.url).map_err(|_| format!("URL de download inválida para {}", game_id))?) };
            if !matches!(source.kind.as_str(), "direct" | "torrent" | "magnet") { continue; }
            if source.kind == "direct" && parsed.as_ref().map(|u| u.scheme()) != Some("https") { continue; }
            if source.kind == "torrent" && parsed.as_ref().map(|u| u.scheme()) != Some("https") { continue; }
            if source.kind == "magnet" && !source.url.starts_with("magnet:") { continue; }
            tx.execute("INSERT INTO download_sources(game_id,repository_id,name,url,kind,size,sha256) VALUES(?1,?2,?3,?4,?5,?6,?7)", params![game_id,id,source.name,source.url,source.kind,source.size,source.sha256]).map_err(|e| e.to_string())?;
        }
        count += 1;
    }
    tx.execute("UPDATE repositories SET last_sync=CURRENT_TIMESTAMP WHERE id=?1", [id]).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(count)
}

#[tauri::command]
fn remove_repository(app: AppHandle, id: i64) -> Result<(), String> {
    let db = connect(&app)?;
    db.execute("DELETE FROM download_sources WHERE repository_id=?1", [id]).map_err(|e| e.to_string())?;
    db.execute("DELETE FROM games WHERE repository_id=?1", [id]).map_err(|e| e.to_string())?;
    db.execute("DELETE FROM repositories WHERE id=?1", [id]).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn list_game_sources(app: AppHandle, game_id: i64) -> Result<Vec<DownloadSource>, String> {
    let db = connect(&app)?;
    let mut s = db.prepare("SELECT ds.id,ds.game_id,ds.repository_id,r.name,ds.name,ds.url,ds.kind,ds.size,ds.sha256 FROM download_sources ds JOIN repositories r ON r.id=ds.repository_id WHERE ds.game_id=?1 ORDER BY r.name,ds.name").map_err(|e| e.to_string())?;
    let rows = s.query_map([game_id], |r| Ok(DownloadSource { id:r.get(0)?,game_id:r.get(1)?,repository_id:r.get(2)?,repository_name:r.get(3)?,name:r.get(4)?,url:r.get(5)?,kind:r.get(6)?,size:r.get(7)?,sha256:r.get(8)? })).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

fn safe_filename(raw: &str) -> String {
    let cleaned: String = raw.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ') { c } else { '_' }).collect();
    let cleaned = cleaned.trim_matches(|c| c == '.' || c == ' ');
    if cleaned.is_empty() { "download.bin".into() } else { cleaned.chars().take(140).collect() }
}

fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() { return candidate; }
    let path = Path::new(name);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("download");
    let ext = path.extension().and_then(|s| s.to_str());
    for n in 2..10000 {
        let next = match ext { Some(ext) => format!("{stem} ({n}).{ext}"), None => format!("{stem} ({n})") };
        let candidate = dir.join(next);
        if !candidate.exists() { return candidate; }
    }
    dir.join(format!("{stem}-download"))
}

async fn download_with_aria2(app: &AppHandle, source_id: i64, game_id: i64, game_title: String, url: String, kind: String, destination: &Path) -> Result<String, String> {
    let resource = app.path().resource_dir().map_err(|e| e.to_string())?;
    #[cfg(target_os="windows")]
    let aria = resource.join("resources").join("aria2c.exe");
    #[cfg(target_os="linux")]
    let aria = resource.join("resources").join("aria2c.exe");
    if !aria.is_file() { return Err("O motor BitTorrent não foi incluído neste pacote. Recompile pelo GitHub Actions.".into()); }
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    drop(listener);
    let token = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos().to_string();
    let mut command = TokioCommand::new(&aria);
    command.kill_on_drop(true);
    let mut child = command
        .arg("--enable-rpc=true").arg("--rpc-listen-all=false").arg(format!("--rpc-listen-port={port}"))
        .arg(format!("--rpc-secret={token}")).arg(format!("--ca-certificate={}", resource.join("resources").join("ca-certificates.crt").to_string_lossy())).arg("--seed-time=0").arg("--continue=true")
        .arg("--summary-interval=0").arg("--console-log-level=error").arg("--quiet=true")
        .arg(format!("--dir={}", destination.to_string_lossy()))
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).stdin(std::process::Stdio::null())
        .spawn().map_err(|e| format!("Não foi possível iniciar o motor de download: {e}"))?;
    let client = reqwest::Client::new();
    let endpoint = format!("http://127.0.0.1:{port}/jsonrpc");
    let mut ready = false;
    for _ in 0..40 {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() { return Err("O motor BitTorrent encerrou inesperadamente".into()); }
        let ping = client.post(&endpoint).json(&serde_json::json!({"jsonrpc":"2.0","id":"vapora","method":"aria2.getVersion","params":[format!("token:{token}")]})).send().await;
        if ping.is_ok() { ready = true; break; }
        sleep(Duration::from_millis(250)).await;
    }
    if !ready { let _ = child.kill().await; return Err("O motor BitTorrent não iniciou o serviço local".into()); }
    let add_payload = if kind == "magnet" {
        serde_json::json!({"jsonrpc":"2.0","id":"vapora-add","method":"aria2.addUri","params":[format!("token:{token}"),[url]]})
    } else if kind == "torrent" {
        let torrent_response = reqwest::get(&url).await.map_err(|e|e.to_string())?.error_for_status().map_err(|e|e.to_string())?;
        if torrent_response.url().scheme() != "https" { return Err("O arquivo torrent redirecionou para uma conexão não segura".into()); }
        let torrent_bytes = torrent_response.bytes().await.map_err(|e|e.to_string())?;
        serde_json::json!({"jsonrpc":"2.0","id":"vapora-add","method":"aria2.addTorrent","params":[format!("token:{token}"),BASE64.encode(torrent_bytes),[],{"dir":destination.to_string_lossy()}]})
    } else {
        serde_json::json!({"jsonrpc":"2.0","id":"vapora-add","method":"aria2.addUri","params":[format!("token:{token}"),[url]]})
    };
    let add = client.post(&endpoint).json(&add_payload).send().await.map_err(|e| e.to_string())?;
    let add_json: serde_json::Value = add.json().await.map_err(|e| e.to_string())?;
    let gid = add_json.get("result").and_then(|v| v.as_str()).ok_or_else(|| add_json.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()).unwrap_or("Falha ao adicionar transferência").to_string())?.to_string();
    let mut final_path = None;
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() { return Err("O motor de download foi encerrado antes de concluir".into()); }
        let status_response = client.post(&endpoint).json(&serde_json::json!({"jsonrpc":"2.0","id":"vapora-status","method":"aria2.tellStatus","params":[format!("token:{token}"),gid, ["status","totalLength","completedLength","downloadSpeed","errorMessage","files"]]})).send().await.map_err(|e| e.to_string())?;
        let payload: serde_json::Value = status_response.json().await.map_err(|e| e.to_string())?;
        let result = payload.get("result").ok_or_else(|| payload.get("error").and_then(|e|e.get("message")).and_then(|m|m.as_str()).unwrap_or("Erro consultando download").to_string())?;
        let status = result.get("status").and_then(|v|v.as_str()).unwrap_or("active");
        let downloaded = result.get("completedLength").and_then(|v|v.as_str()).and_then(|v|v.parse::<u64>().ok()).unwrap_or(0);
        let total = result.get("totalLength").and_then(|v|v.as_str()).and_then(|v|v.parse::<u64>().ok());
        if let Some(files) = result.get("files").and_then(|v|v.as_array()) {
            final_path = files.first().and_then(|f|f.get("path")).and_then(|v|v.as_str()).map(str::to_string).or(final_path);
        }
        let _ = app.emit("download-progress", DownloadEvent { source_id,game_id,game_title:game_title.clone(),file_name:final_path.as_ref().and_then(|p|Path::new(p).file_name()).and_then(|n|n.to_str()).unwrap_or("Transferência").to_string(),downloaded,total,status:if status=="complete" {"completed".into()} else if status=="error" {"failed".into()} else {"downloading".into()},path:final_path.clone(),error:result.get("errorMessage").and_then(|v|v.as_str()).map(str::to_string) });
        match status {
            "complete" => break,
            "error" => { let msg = result.get("errorMessage").and_then(|v|v.as_str()).unwrap_or("Falha no download BitTorrent").to_string(); let _ = child.kill().await; return Err(msg); },
            "removed" => { let _ = child.kill().await; return Err("Transferência removida".into()); },
            _ => sleep(Duration::from_secs(1)).await,
        }
    }
    let _ = client.post(&endpoint).json(&serde_json::json!({"jsonrpc":"2.0","id":"vapora-stop","method":"aria2.shutdown","params":[format!("token:{token}")]})).send().await;
    let _ = child.wait().await;
    final_path.ok_or_else(|| "Download concluído, mas o caminho do arquivo não foi retornado".into())
}

#[tauri::command]
async fn download_game_source(app: AppHandle, source_id: i64, destination_dir: String) -> Result<String, String> {
    let (game_id, game_title, url, expected_sha, kind) = {
        let db = connect(&app)?;
        db.query_row("SELECT ds.game_id,g.title,ds.url,ds.sha256,ds.kind FROM download_sources ds JOIN games g ON g.id=ds.game_id WHERE ds.id=?1", [source_id], |r| Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,Option<String>>(3)?,r.get::<_,String>(4)?))).map_err(|e| e.to_string())?
    };
    let dir = PathBuf::from(&destination_dir);
    tokio::fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;
    if kind == "magnet" || kind == "torrent" {
        if kind == "magnet" && !url.starts_with("magnet:") { return Err("Magnet URI inválida".into()); }
        if kind == "torrent" && reqwest::Url::parse(&url).map(|u|u.scheme()!= "https").unwrap_or(true) { return Err("O torrent precisa usar HTTPS".into()); }
        return download_with_aria2(&app,source_id,game_id,game_title,url,kind,&dir).await;
    }
    let parsed = reqwest::Url::parse(&url).map_err(|_| "Endereço de download inválido".to_string())?;
    if parsed.scheme() != "https" { return Err("Downloads precisam usar HTTPS".into()); }
    let raw_name = parsed.path_segments().and_then(|mut s| s.next_back()).filter(|s| !s.is_empty()).unwrap_or("download.bin");
    let file_name = safe_filename(raw_name);
    let path = unique_path(&dir, &file_name);
    let client = reqwest::Client::builder().user_agent("Vapora/0.2").redirect(reqwest::redirect::Policy::limited(5)).build().map_err(|e| e.to_string())?;
    let response = client.get(&url).send().await.map_err(|e| e.to_string())?.error_for_status().map_err(|e| e.to_string())?;
    if response.url().scheme() != "https" { return Err("O servidor redirecionou para uma conexão não segura".into()); }
    let total = response.content_length();
    let temp_path = path.with_file_name(format!("{file_name}.part"));
    let mut file = tokio::fs::File::create(&temp_path).await.map_err(|e| e.to_string())?;
    let mut stream = response.bytes_stream();
    let mut downloaded = 0u64;
    let mut hasher = Sha256::new();
    let mut last_emit = 0u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        hasher.update(&chunk);
        downloaded += chunk.len() as u64;
        if downloaded.saturating_sub(last_emit) >= 1024 * 512 {
            let _ = app.emit("download-progress", DownloadEvent { source_id,game_id,game_title:game_title.clone(),file_name:file_name.clone(),downloaded,total,status:"downloading".into(),path:None,error:None });
            last_emit = downloaded;
        }
    }
    file.flush().await.map_err(|e| e.to_string())?;
    drop(file);
    if let Some(expected) = expected_sha {
        let actual = format!("{:x}", hasher.finalize());
        if !actual.eq_ignore_ascii_case(expected.trim()) {
            let _ = tokio::fs::remove_file(&temp_path).await;
            let reason = "O arquivo baixado não corresponde ao SHA-256 informado pelo repositório".to_string();
            let _ = app.emit("download-progress", DownloadEvent { source_id,game_id,game_title,file_name,downloaded,total,status:"failed".into(),path:None,error:Some(reason.clone()) });
            return Err(reason);
        }
    }
    tokio::fs::rename(&temp_path, &path).await.map_err(|e| e.to_string())?;
    let path_string = path.to_string_lossy().to_string();
    let _ = app.emit("download-progress", DownloadEvent { source_id,game_id,game_title,file_name,downloaded,total,status:"completed".into(),path:Some(path_string.clone()),error:None });
    Ok(path_string)
}

#[tauri::command]
fn extract_zip(archive_path: String, destination_dir: String) -> Result<String, String> {
    let archive_file = std::fs::File::open(&archive_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(archive_file).map_err(|e| format!("Arquivo ZIP inválido: {e}"))?;
    let base = PathBuf::from(destination_dir);
    std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let canonical_base = base.canonicalize().map_err(|e| e.to_string())?;
    if archive.len() > 200_000 { return Err("O ZIP contém entradas demais".into()); }
    let mut expanded_total = 0u64;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        expanded_total = expanded_total.saturating_add(entry.size());
        if expanded_total > 500 * 1024 * 1024 * 1024 { return Err("O conteúdo descompactado excede o limite de segurança de 500 GiB".into()); }
        let Some(relative) = entry.enclosed_name() else { return Err("O ZIP contém um caminho inseguro".into()); };
        let out = canonical_base.join(relative);
        if entry.is_dir() { std::fs::create_dir_all(&out).map_err(|e| e.to_string())?; continue; }
        if out.exists() { return Err(format!("O arquivo já existe e não será sobrescrito: {}", out.display())); }
        if let Some(parent) = out.parent() { std::fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
        let mut target = std::fs::File::create(&out).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut target).map_err(|e| e.to_string())?;
    }
    Ok(canonical_base.to_string_lossy().to_string())
}

#[tauri::command]
fn open_steam_store(appid: u64) -> Result<(), String> {
    let url = format!("https://store.steampowered.com/app/{appid}/");
    #[cfg(target_os="windows")]
    Command::new("cmd").args(["/C", "start", "", url.as_str()]).spawn().map_err(|e| e.to_string())?;
    #[cfg(target_os="linux")]
    Command::new("xdg-open").arg(&url).spawn().map_err(|e| e.to_string())?;
    #[cfg(target_os="macos")]
    Command::new("open").arg(&url).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn open_steam_install(appid: u64) -> Result<(), String> {
    let uri = format!("steam://install/{appid}");
    #[cfg(target_os="windows")]
    Command::new("cmd").args(["/C", "start", "", uri.as_str()]).spawn().map_err(|e| e.to_string())?;
    #[cfg(target_os="linux")]
    Command::new("xdg-open").arg(&uri).spawn().map_err(|e| e.to_string())?;
    #[cfg(target_os="macos")]
    Command::new("open").arg(&uri).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn launch_game(executable: String) -> Result<(), String> {
    let path = PathBuf::from(&executable);
    if !path.is_file() { return Err("Executável não encontrado. Confira o caminho nas propriedades do jogo.".into()); }
    Command::new(path).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn reveal_path(path: String) -> Result<(), String> {
    let p = PathBuf::from(path);
    if !p.exists() { return Err("Caminho não encontrado".into()); }
    #[cfg(target_os="windows")]
    Command::new("explorer").arg(p).spawn().map_err(|e| e.to_string())?;
    #[cfg(target_os="linux")]
    Command::new("xdg-open").arg(p).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![fetch_steam_game_details,fetch_steam_game_reviews,open_steam_store,open_steam_install,list_games,list_catalog_games,add_game,remove_game,list_repositories,add_repository,sync_repository,remove_repository,list_game_sources,download_game_source,extract_zip,launch_game,reveal_path])
        .run(tauri::generate_context!())
        .expect("error while running Vapora");
}
