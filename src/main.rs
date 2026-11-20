mod cli;
mod display;
mod playback;
mod project;
mod render;

fn main() {
    if let Err(error) = cli::run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
