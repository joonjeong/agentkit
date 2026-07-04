fn main() {
    if let Err(error) = ops_session::run(std::env::args_os()) {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
