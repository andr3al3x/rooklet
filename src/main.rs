mod auth;
mod cli;
mod json;
mod tui;

fn main() {
    if let Err(error) = cli::run() {
        eprintln!("xield: {error:#}");
        std::process::exit(1);
    }
}
