use std::io::{self, Read};

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut input = Vec::new();
    if io::stdin().read_to_end(&mut input).is_err() {
        return std::process::ExitCode::from(2);
    }
    let (output, exit) = boxology_telegram::execute(&args, &input);
    println!("{output}");
    std::process::ExitCode::from(exit as u8)
}
