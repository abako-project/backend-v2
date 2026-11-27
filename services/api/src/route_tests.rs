//! The router and the public contract list the same routes.
use serde_json::Value;
use std::collections::BTreeSet;

const METHODS: [&str; 5] = ["get", "post", "put", "patch", "delete"];

/// A path with its parameters' names removed: the two sides name them freely.
fn shape(path: &str) -> String {
    let mut shape = String::new();
    let mut inside = false;
    for character in path.chars() {
        match character {
            '{' => {
                inside = true;
                shape.push_str("{}");
            }
            '}' => inside = false,
            _ if !inside => shape.push(character),
            _ => {}
        }
    }
    shape
}

/// Every `.route("<path>", <methods>)` of the router's source.
fn routed() -> BTreeSet<(String, String)> {
    let source = include_str!("http.rs");
    let mut routes = BTreeSet::new();
    for call in source.split(".route(").skip(1) {
        let Some(path) = call.split('"').nth(1) else {
            continue;
        };
        if !path.starts_with("/api/") {
            continue;
        }
        for method in METHODS {
            let token = format!("{method}(");
            let used = call.match_indices(&token).any(|(at, _)| {
                call[..at]
                    .chars()
                    .next_back()
                    .is_none_or(|before| !before.is_alphanumeric() && before != '_')
            });
            if used {
                routes.insert((method.to_owned(), shape(path)));
            }
        }
    }
    routes
}

#[test]
fn the_router_and_the_contract_list_the_same_routes() -> Result<(), Box<dyn std::error::Error>> {
    let document: Value = serde_json::from_str(include_str!("../../../contracts/openapi.json"))?;
    let paths = document["paths"].as_object().ok_or("no paths")?;
    let documented: BTreeSet<(String, String)> = paths
        .iter()
        .flat_map(|(path, item)| {
            METHODS
                .iter()
                .filter(|method| item.get(**method).is_some())
                .map(|method| ((*method).to_owned(), shape(path)))
                .collect::<Vec<_>>()
        })
        .collect();
    let routed = routed();
    let undocumented: Vec<_> = routed.difference(&documented).collect();
    let unrouted: Vec<_> = documented.difference(&routed).collect();
    assert!(
        undocumented.is_empty() && unrouted.is_empty(),
        "routed but not documented: {undocumented:?}; documented but not routed: {unrouted:?}"
    );
    assert_eq!(documented.len(), 96);
    Ok(())
}
