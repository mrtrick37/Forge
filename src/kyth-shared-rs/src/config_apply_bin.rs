//! Closed contract for the aggregate user-configuration request.
//!
//! The former aggregate launcher is absent from the checkout. This first
//! native slice intentionally exposes planning only: callers can validate and
//! inspect a request without causing a desktop or network side effect.

use serde::{Deserialize, Serialize};
use std::io::{self, Read};

#[derive(Debug, Deserialize)]
struct Request {
    operation: String,
    #[serde(default)]
    profile: Option<String>,
}

#[derive(Debug, Serialize)]
struct Plan<'a> {
    operation: &'a str,
    mode: &'static str,
    profile: Option<String>,
    side_effects: Vec<&'static str>,
}

fn main() {
    let mut input = String::new();
    if io::stdin().read_to_string(&mut input).is_err() {
        eprintln!("kyth-config-apply: could not read request from stdin");
        std::process::exit(64);
    }
    let Ok(request) = serde_json::from_str::<Request>(&input) else {
        eprintln!("kyth-config-apply: request must be a JSON object on stdin");
        std::process::exit(64);
    };
    let effects = match request.operation.as_str() {
        "desktop" => vec!["write Plasma desktop projection"],
        "display" => vec!["write display projection", "bounded kscreen readback"],
        "input" => vec!["write Xorg input projection", "write 30-second TTL"],
        "network" => vec!["write network drop-in"],
        "pipewire" => vec![
            "write PipeWire drop-in and environment map",
            "do not reload PipeWire",
        ],
        "rgb" => vec!["invoke fixed OpenRGB/liquidctl operations"],
        "tailscale" => vec!["invoke tailscale up with validated settings"],
        "role" => vec!["write role layout projection", "apply declarative preset"],
        _ => {
            eprintln!("kyth-config-apply: unknown operation; allowed: desktop display input network pipewire rgb tailscale role");
            std::process::exit(64);
        }
    };
    if request.operation == "role" {
        if !matches!(
            request.profile.as_deref(),
            Some("everyday" | "gaming" | "dev" | "creator")
        ) {
            eprintln!("kyth-config-apply: role requires profile everyday|gaming|dev|creator");
            std::process::exit(64);
        }
    }
    let output = serde_json::to_string(&Plan {
        operation: &request.operation,
        mode: "plan",
        profile: request.profile,
        side_effects: effects,
    })
    .unwrap();
    println!("{output}");
}

#[cfg(test)]
mod tests {
    use super::Request;

    #[test]
    fn request_contract_is_closed_and_role_profile_is_explicit() {
        let request: Request =
            serde_json::from_str(r#"{"operation":"role","profile":"gaming"}"#).unwrap();
        assert_eq!(request.operation, "role");
        assert_eq!(request.profile.as_deref(), Some("gaming"));
    }
}
