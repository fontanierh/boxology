use std::io::{self, Read};

fn main() -> std::process::ExitCode {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let mut input = Vec::new();
    if io::stdin().take(65_537).read_to_end(&mut input).is_err() {
        return std::process::ExitCode::from(2);
    }
    let mut stdout = io::stdout().lock();
    std::process::ExitCode::from(boxology_cli::run_telegram(&args, &input, &mut stdout) as u8)
}
