fn main() {
    if let Err(error) = agent_session::run(std::env::args_os()) {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
