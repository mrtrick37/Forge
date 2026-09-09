//! Native read-only printer discovery.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() && args != ["--json"] {
        eprintln!("Usage: kyth-print-check [--json]");
        std::process::exit(2);
    }
    let printers = kyth_shared::system::printing::ipp_discover();
    if args == ["--json"] {
        println!(
            "{}",
            serde_json::to_string_pretty(&printers).expect("printer list serializes")
        );
    } else if printers.is_empty() {
        println!("kyth-print-check: no printers or discovery command missing");
    } else {
        for printer in printers {
            println!("{printer}");
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn empty_discovery_has_stable_json_shape() {
        assert_eq!(
            serde_json::to_string_pretty(&Vec::<String>::new()).unwrap(),
            "[]"
        );
    }
}
