mod auth;
mod cli;
mod json;
mod tui;

fn main() {
    if let Err(error) = cli::run() {
        eprintln!(
            "rooklet: {}",
            rooklet::clean_multiline(&format!("{error:#}"))
        );
        std::process::exit(1);
    }
}
