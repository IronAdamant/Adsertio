fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let name = std::env::args().next().unwrap_or_default();
    let as_mcp = name
        .rsplit(['/', '\\'])
        .next()
        .is_some_and(|n| n.starts_with("adsertio-mcp"));
    std::process::exit(adsertio::cli::run(&args, as_mcp));
}
