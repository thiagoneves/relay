fn main() {
    match relay::run() {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("relay: {e:#}");
            std::process::exit(1);
        }
    }
}
