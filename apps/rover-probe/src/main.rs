fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let outcome = rover_probe::run(&args);

    if !outcome.stdout.is_empty() {
        println!("{}", outcome.stdout);
    }

    if !outcome.stderr.is_empty() {
        eprintln!("{}", outcome.stderr);
    }

    std::process::exit(outcome.exit_code);
}
