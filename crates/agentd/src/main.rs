fn main() {
    if let Err(error) = agentd::run(std::env::args_os()) {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
