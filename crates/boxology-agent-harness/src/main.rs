fn main() {
    std::process::exit(agent_harness::main_entry(
        std::env::args_os().skip(1).collect(),
        std::io::stdin().lock(),
        std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    ));
}
