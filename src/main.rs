mod config;
mod models;
mod printer;
mod server;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    server::run()
}
