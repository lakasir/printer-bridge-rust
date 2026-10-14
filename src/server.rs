use crate::config::{Settings, load_settings, save_settings};
use crate::models::PrintRequest;
use crate::models::SettingsUpdate;
use crate::printer::{get_printers, print_job, render_print_job};
use serde::Serialize;
use std::io::Read;
use std::time::Duration;
use tiny_http::{Header, Request, Response, Server, StatusCode};

const MAX_REQUEST_SIZE: u64 = 1_048_576;

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

pub(crate) fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
