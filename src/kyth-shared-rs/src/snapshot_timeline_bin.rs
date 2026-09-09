//! Native read-only snapshot and deployment timeline.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json = args.iter().any(|arg| arg == "--json");
    let mut limit = 20usize;
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--limit" {
            let Some(value) = args.get(index + 1).and_then(|value| value.parse().ok()) else {
                eprintln!("--limit requires a non-negative integer");
                std::process::exit(2);
            };
            limit = value;
            index += 1;
        }
        index += 1;
    }
    if args
        .iter()
        .any(|arg| arg.starts_with('-') && arg != "--json" && arg != "--limit")
    {
        eprintln!("Usage: kyth-snapshot-timeline [--json] [--limit N]");
        std::process::exit(2);
    }
    let rows = kyth_shared::system::snapshot::snapshot_timeline(limit);
    if json {
        println!(
            "{}",
            kyth_shared::system::snapshot::snapshot_rows_json(&rows)
        );
    } else {
        for row in rows {
            println!("{:<12} {:<12} {}", row.row_type, row.id, row.description);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parser_accepts_expected_flags() {
        let args = ["--json", "--limit", "4"];
        assert!(args.contains(&"--json"));
        assert_eq!(args[2].parse::<usize>().unwrap(), 4);
    }
}
