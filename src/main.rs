fn main() {
    if let Err(err) = shelf::cli::run() {
        eprintln!("shelf: {err}");
        std::process::exit(1);
    }
}
