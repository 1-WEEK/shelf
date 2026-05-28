fn main() {
    if let Err(err) = shelf::root_cli::run() {
        eprintln!("shelf-root: {err}");
        std::process::exit(1);
    }
}
