use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Command;
#[cfg(unix)]
use std::process::Stdio;
use std::time::Duration;
use tiny_http::{Header, Request, Response, Server, StatusCode};

const DEFAULT_PAPER_WIDTH: u16 = 48;
const MAX_REQUEST_SIZE: u64 = 1_048_576;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Settings {
    printer_name: String,
    paper_width: u16,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            printer_name: String::new(),
            paper_width: DEFAULT_PAPER_WIDTH,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrintRequest {
    text: Option<String>,
    items: Option<Vec<String>>,
    image: Option<String>,
    image_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingsUpdate {
    printer_name: String,
    paper_width: u16,
}

fn settings_path() -> Result<PathBuf, String> {
    let config_dir = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
    }
    .ok_or_else(|| "Could not determine the user configuration directory".to_string())?;

    Ok(config_dir
        .join("lakasir")
        .join("printer-bridge")
        .join("settings.json"))
}

fn load_settings() -> Result<Settings, String> {
    let path = settings_path()?;
    match fs::read_to_string(path) {
        Ok(contents) => serde_json::from_str(&contents)
            .map_err(|error| format!("Could not read settings: {error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(error) => Err(format!("Could not read settings: {error}")),
    }
}

fn save_settings(settings: &Settings) -> Result<(), String> {
    let path = settings_path()?;
    let parent = path
        .parent()
        .ok_or_else(|| "Could not determine the settings directory".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create settings directory: {error}"))?;
    let contents = serde_json::to_vec_pretty(settings)
        .map_err(|error| format!("Could not encode settings: {error}"))?;
    fs::write(path, contents).map_err(|error| format!("Could not save settings: {error}"))
}

fn render_print_job(data: PrintRequest, paper_width: usize) -> Result<String, String> {
    if data.image.is_some() || data.image_url.is_some() {
        return Err("Image printing is not supported by this Rust bridge".to_string());
    }

    let mut output = String::new();
    if let Some(text) = data.text
        && !text.is_empty()
    {
        output.push_str(&wrap_text(&text, paper_width));
    }
    if let Some(items) = data.items {
        for item in items {
            output.push_str(&wrap_text(&item, paper_width));
        }
    }
    if output.is_empty() {
        return Err("Print request must include text or items".to_string());
    }

    Ok(output)
}

fn wrap_text(text: &str, width: usize) -> String {
    let width = width.max(1);
    let mut output = String::new();
    for line in text.split('\n') {
        let characters: Vec<char> = line.chars().collect();
        if characters.is_empty() {
            output.push('\n');
            continue;
        }
        for chunk in characters.chunks(width) {
            output.extend(chunk);
            output.push('\n');
        }
    }
    output
}

#[cfg(unix)]
fn run_printer_command(program: &str, args: &[&str], contents: &[u8]) -> Result<(), String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Could not start {program}: {error}"))?;

    child
        .stdin
        .take()
        .ok_or_else(|| format!("Could not write to {program}"))?
        .write_all(contents)
        .map_err(|error| format!("Could not send print job to {program}: {error}"))?;

    let output = child
        .wait_with_output()
        .map_err(|error| format!("Could not finish print job: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Printer command failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn print_job(settings: &Settings, contents: &str) -> Result<(), String> {
    let bytes = contents.as_bytes();
    if settings.printer_name.is_empty() {
        match run_printer_command("lp", &[], bytes) {
            Err(error) if error.contains("Could not start lp") => {
                run_printer_command("lpr", &[], bytes)
            }
            result => result,
        }
    } else {
        match run_printer_command("lp", &["-d", &settings.printer_name], bytes) {
            Err(error) if error.contains("Could not start lp") => {
                run_printer_command("lpr", &["-P", &settings.printer_name], bytes)
            }
            result => result,
        }
    }
}

#[cfg(windows)]
fn print_job(settings: &Settings, contents: &str) -> Result<(), String> {
    use std::fs::OpenOptions;
    use std::time::{SystemTime, UNIX_EPOCH};

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let path =
        std::env::temp_dir().join(format!("printer-bridge-{}-{stamp}.txt", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| format!("Could not create print job: {error}"))?;
    file.write_all(b"\xef\xbb\xbf")
        .map_err(|error| format!("Could not encode print job: {error}"))?;
    file.write_all(contents.as_bytes())
        .map_err(|error| format!("Could not write print job: {error}"))?;
    drop(file);

    let result = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$contents = Get-Content -Raw -LiteralPath $env:PRINTER_BRIDGE_FILE; if ($env:PRINTER_BRIDGE_PRINTER) { $contents | Out-Printer -Name $env:PRINTER_BRIDGE_PRINTER } else { $contents | Out-Printer }",
        ])
        .env("PRINTER_BRIDGE_FILE", &path)
        .env("PRINTER_BRIDGE_PRINTER", &settings.printer_name)
        .output()
        .map_err(|error| format!("Could not start Windows printer command: {error}"))
        .and_then(|output| {
            if output.status.success() {
                Ok(())
            } else {
                Err(format!(
                    "Printer command failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ))
            }
        });

    let _ = fs::remove_file(path);
    result
}

#[cfg(not(any(unix, windows)))]
fn print_job(_settings: &Settings, _contents: &str) -> Result<(), String> {
    Err("Printing is not supported on this operating system".to_string())
}

#[cfg(unix)]
fn get_printers() -> Result<Vec<String>, String> {
    let output = Command::new("lpstat")
        .arg("-p")
        .output()
        .map_err(|error| format!("Could not list printers with lpstat: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Could not list printers: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.strip_prefix("printer "))
        .filter_map(|line| line.split_whitespace().next())
        .map(str::to_string)
        .collect())
}

#[cfg(windows)]
fn get_printers() -> Result<Vec<String>, String> {
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-Printer | ForEach-Object { $_.Name }",
        ])
        .output()
        .map_err(|error| format!("Could not list printers with PowerShell: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Could not list printers: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect())
}

#[cfg(not(any(unix, windows)))]
fn get_printers() -> Result<Vec<String>, String> {
    Err("Printer discovery is not supported on this operating system".to_string())
}

fn json_response(status: u16, value: &impl Serialize) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec());
    Response::from_data(body)
        .with_status_code(StatusCode(status))
        .with_header(Header::from_bytes("Content-Type", "application/json").unwrap())
        .with_header(Header::from_bytes("Access-Control-Allow-Origin", "*").unwrap())
        .with_header(
            Header::from_bytes("Access-Control-Allow-Methods", "GET, POST, OPTIONS").unwrap(),
        )
        .with_header(Header::from_bytes("Access-Control-Allow-Headers", "Content-Type").unwrap())
}

fn error_response(status: u16, message: String) -> Response<std::io::Cursor<Vec<u8>>> {
    json_response(
        status,
        &serde_json::json!({ "status": "error", "message": message }),
    )
}

fn read_request_body(request: &mut Request) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    request
        .as_reader()
        .take(MAX_REQUEST_SIZE + 1)
        .read_to_end(&mut body)
        .map_err(|error| format!("Could not read request: {error}"))?;
    if body.len() as u64 > MAX_REQUEST_SIZE {
        return Err("Request body exceeds the 1 MB limit".to_string());
    }
    Ok(body)
}

fn handle_request(mut request: Request) {
    let method = request.method().as_str().to_string();
    let path = request.url().to_string();

    let response = if method == "OPTIONS" {
        json_response(204, &serde_json::json!({}))
    } else {
        match (method.as_str(), path.as_str()) {
            ("GET", "/") => json_response(
                200,
                &serde_json::json!({ "status": "ok", "service": "printer-bridge" }),
            ),
            ("GET", "/settings") => match load_settings() {
                Ok(settings) => json_response(200, &settings),
                Err(error) => error_response(500, error),
            },
            ("POST", "/settings") => match read_request_body(&mut request).and_then(|body| {
                serde_json::from_slice::<SettingsUpdate>(&body).map_err(|e| e.to_string())
            }) {
                Ok(update) if update.paper_width > 0 => {
                    let settings = Settings {
                        printer_name: update.printer_name,
                        paper_width: update.paper_width,
                    };
                    match save_settings(&settings) {
                        Ok(()) => json_response(200, &settings),
                        Err(error) => error_response(500, error),
                    }
                }
                Ok(_) => error_response(400, "paperWidth must be greater than zero".to_string()),
                Err(error) => error_response(400, format!("Invalid settings: {error}")),
            },
            ("GET", "/printers") => match get_printers() {
                Ok(printers) => json_response(
                    200,
                    &serde_json::json!({ "printers": printers.into_iter().map(|name| serde_json::json!({ "name": name })).collect::<Vec<_>>() }),
                ),
                Err(error) => error_response(500, error),
            },
            ("POST", "/print") => {
                let print_data = read_request_body(&mut request).and_then(|body| {
                    serde_json::from_slice::<PrintRequest>(&body).map_err(|e| e.to_string())
                });
                match print_data {
                    Err(error) => error_response(400, error),
                    Ok(data) => match load_settings() {
                        Err(error) => error_response(500, error),
                        Ok(settings) => {
                            match render_print_job(data, settings.paper_width as usize) {
                                Err(error) => error_response(400, error),
                                Ok(job) => match print_job(&settings, &job) {
                                    Ok(()) => json_response(
                                        200,
                                        &serde_json::json!({ "status": "success", "message": "Print job completed" }),
                                    ),
                                    Err(error) => error_response(500, error),
                                },
                            }
                        }
                    },
                }
            }
            _ => error_response(404, "Not found".to_string()),
        }
    };

    let _ = request.respond(response);
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let port = std::env::var("HTTP_PORT")
        .unwrap_or_else(|_| "5463".to_string())
        .parse::<u16>()?;
    let server = Server::http(format!("127.0.0.1:{port}"))?;
    println!("Printer bridge listening on http://127.0.0.1:{port}");

    loop {
        if let Some(request) = server.recv_timeout(Duration::from_millis(250))? {
            handle_request(request);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_use_the_existing_defaults_and_json_field_names() {
        let settings = Settings::default();
        assert_eq!(settings.printer_name, "");
        assert_eq!(settings.paper_width, 48);
        assert_eq!(
            serde_json::to_string(&settings).unwrap(),
            r#"{"printerName":"","paperWidth":48}"#
        );
    }

    #[test]
    fn print_job_combines_text_and_items_in_order() {
        let job = render_print_job(
            PrintRequest {
                text: Some("Order #1".to_string()),
                items: Some(vec!["Item A".to_string(), "Total".to_string()]),
                image: None,
                image_url: None,
            },
            48,
        )
        .unwrap();
        assert_eq!(job, "Order #1\nItem A\nTotal\n");
    }

    #[test]
    fn print_job_wraps_at_the_configured_character_width() {
        let job = render_print_job(
            PrintRequest {
                text: Some("12345".to_string()),
                items: None,
                image: None,
                image_url: None,
            },
            3,
        )
        .unwrap();
        assert_eq!(job, "123\n45\n");
    }

    #[test]
    fn print_job_rejects_empty_and_image_only_requests() {
        assert!(
            render_print_job(
                PrintRequest {
                    text: None,
                    items: None,
                    image: None,
                    image_url: None,
                },
                48,
            )
            .is_err()
        );
        assert!(
            render_print_job(
                PrintRequest {
                    text: None,
                    items: None,
                    image: Some("base64".to_string()),
                    image_url: None,
                },
                48,
            )
            .unwrap_err()
            .contains("Image printing")
        );
    }
}
