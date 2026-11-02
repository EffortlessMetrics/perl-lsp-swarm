//! Contract pins for semantic packet E (#6599): spec, routing policy, and crate note.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

const INCLUDED_ISSUES: [u32; 15] =
    [4879, 4736, 4737, 4775, 4770, 4810, 3651, 3700, 3708, 2660, 2582, 2559, 2543, 2411, 2210];

const FOLLOW_UPS: [(&str, u32); 3] =
    [("compatibility_ledger", 4736), ("fact_taxonomy", 2543), ("capability_backlog", 2559)];

fn project_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

#[derive(Debug)]
struct RouteRow {
    issue: u32,
    follow_up: String,
}

fn parse_routing_policy(text: &str) -> Result<Vec<RouteRow>, Box<dyn std::error::Error>> {
    let doc: toml::Table = text.parse()?;
    let routes = doc.get("routes").and_then(|v| v.as_array()).ok_or("missing [[routes]] table")?;
    let mut rows = Vec::with_capacity(routes.len());
    for entry in routes {
        let table = entry.as_table().ok_or("route entry must be a table")?;
        let issue =
            table.get("issue").and_then(|v| v.as_integer()).ok_or("route.issue must be integer")?
                as u32;
        let follow_up = table
            .get("follow_up")
            .and_then(|v| v.as_str())
            .ok_or("route.follow_up must be string")?
            .to_string();
        rows.push(RouteRow { issue, follow_up });
    }
    Ok(rows)
}

#[test]
fn packet_e_spec_documents_unsupported_facts_and_transitions()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let spec =
        fs::read_to_string(root.join("docs/specs/PLSP-SPEC-0039-semantic-contract-packet-e.md"))?;
    for needle in [
        "## Contract — Unsupported facts",
        "## Contract — Parser-to-fact transitions",
        "PLSP-SPEC-0038",
        "PLSP-SPEC-0030",
        "parser_to_semantic_contract",
        "packet-e-issue-routing.v1.toml",
    ] {
        assert!(spec.contains(needle), "spec missing {needle}");
    }
    Ok(())
}

#[test]
fn packet_e_routing_covers_every_included_issue_once() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let policy = fs::read_to_string(root.join("policy/semantic/packet-e-issue-routing.v1.toml"))?;
    assert!(
        policy.contains("schema_version = \"semantic_packet_e_issue_routing.v1\""),
        "routing policy schema_version"
    );
    let rows = parse_routing_policy(&policy)?;
    let mut seen = BTreeSet::new();
    for row in &rows {
        assert!(seen.insert(row.issue), "duplicate route for issue {}", row.issue);
        assert!(
            FOLLOW_UPS.iter().any(|(id, _)| *id == row.follow_up),
            "issue {} has unknown follow_up {}",
            row.issue,
            row.follow_up
        );
    }
    for issue in INCLUDED_ISSUES {
        assert!(seen.contains(&issue), "missing route for issue {issue}");
    }
    assert_eq!(seen.len(), INCLUDED_ISSUES.len());
    Ok(())
}

#[test]
fn packet_e_routing_follow_up_owner_issues_are_declared() -> Result<(), Box<dyn std::error::Error>>
{
    let root = project_root()?;
    let policy = fs::read_to_string(root.join("policy/semantic/packet-e-issue-routing.v1.toml"))?;
    let doc: toml::Table = policy.parse()?;
    let follow_ups =
        doc.get("follow_ups").and_then(|v| v.as_array()).ok_or("missing [[follow_ups]]")?;
    let mut owners = BTreeMap::new();
    for entry in follow_ups {
        let table = entry.as_table().ok_or("follow_up must be table")?;
        let id = table.get("id").and_then(|v| v.as_str()).ok_or("follow_up.id")?;
        let owner =
            table.get("owner_issue").and_then(|v| v.as_integer()).ok_or("owner_issue")? as u32;
        owners.insert(id.to_string(), owner);
    }
    for (id, owner) in FOLLOW_UPS {
        assert_eq!(owners.get(id), Some(&owner), "follow_up owner for {id}");
    }
    Ok(())
}

#[test]
fn packet_e_harness_issues_route_to_compatibility_ledger() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let policy = fs::read_to_string(root.join("policy/semantic/packet-e-issue-routing.v1.toml"))?;
    let rows = parse_routing_policy(&policy)?;
    let routes: BTreeMap<u32, String> = rows.into_iter().map(|row| (row.issue, row.follow_up)).collect();
    for issue in [3651_u32, 3700, 3708, 4737] {
        assert_eq!(
            routes.get(&issue).map(String::as_str),
            Some("compatibility_ledger"),
            "issue {issue} must route to compatibility_ledger"
        );
    }
    Ok(())
}

#[test]
fn parser_to_semantic_contract_module_is_wired_in_semantic_facts()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let lib = fs::read_to_string(root.join("crates/perl-semantic-facts/src/lib.rs"))?;
    assert!(
        lib.contains("mod parser_to_semantic_contract;"),
        "perl-semantic-facts must retain the internal contract note module"
    );
    assert!(
        lib.contains("PLSP-SPEC-0039"),
        "crate root docs must point at the packet E spec"
    );
    let module = fs::read_to_string(
        root.join("crates/perl-semantic-facts/src/parser_to_semantic_contract.rs"),
    )?;
    assert!(module.contains("FOLLOW_UP_COMPATIBILITY_LEDGER"));
    assert!(module.contains("Unsupported facts"));
    Ok(())
}
