use crate::config::Settings;
use crate::models::PrintRequest;
#[cfg(windows)]
use std::fs;
use std::process::Command;

#[cfg(any(unix, windows))]
use std::io::Write;
#[cfg(unix)]
use std::process::Stdio;

pub(crate) fn render_print_job(data: PrintRequest, paper_width: usize) -> Result<String, String> {
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
pub(crate) fn print_job(settings: &Settings, contents: &str) -> Result<(), String> {
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
pub(crate) fn print_job(settings: &Settings, contents: &str) -> Result<(), String> {
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
pub(crate) fn print_job(_settings: &Settings, _contents: &str) -> Result<(), String> {
    Err("Printing is not supported on this operating system".to_string())
}

#[cfg(unix)]
pub(crate) fn get_printers() -> Result<Vec<String>, String> {
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
pub(crate) fn get_printers() -> Result<Vec<String>, String> {
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
pub(crate) fn get_printers() -> Result<Vec<String>, String> {
    Err("Printer discovery is not supported on this operating system".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

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
