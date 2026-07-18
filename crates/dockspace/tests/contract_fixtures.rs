use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, PartialEq)]
struct ContractFixture {
    schema_version: u32,
    scenarios: Vec<Scenario>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
struct Scenario {
    id: String,
    provenance: Provenance,
    initial_workspace: serde_json::Value,
    sequenced_inputs: Vec<serde_json::Value>,
    expected_outcome: serde_json::Value,
    expected_canonical_workspace: serde_json::Value,
    expected_item_multiset: Vec<u64>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
struct Provenance {
    project: String,
    revision: String,
    path: String,
    test: String,
    license: String,
}

fn decode_fixture(input: &str) -> Result<ContractFixture, String> {
    let fixture: ContractFixture =
        serde_json::from_str(input).map_err(|error| error.to_string())?;
    if fixture.schema_version != dockspace::CONTRACT_VERSION {
        return Err(format!(
            "unsupported contract fixture version: {}",
            fixture.schema_version
        ));
    }
    Ok(fixture)
}

#[test]
fn behavior_contract_is_versioned_well_formed_and_canonical() {
    let fixture = decode_fixture(include_str!("fixtures/docking_contract_v1.json"))
        .expect("the repository-owned fixture must be valid");
    assert!(!fixture.scenarios.is_empty());

    let mut ids = BTreeSet::new();
    for scenario in &fixture.scenarios {
        assert!(!scenario.id.trim().is_empty());
        assert!(ids.insert(&scenario.id), "scenario IDs must be unique");
        assert!(
            matches!(
                scenario.provenance.project.as_str(),
                "current" | "open-gpui" | "dear-imgui" | "dockview"
            ),
            "unknown fixture source: {}",
            scenario.provenance.project
        );
        assert!(!scenario.provenance.revision.trim().is_empty());
        assert!(!scenario.provenance.path.trim().is_empty());
        assert!(!scenario.provenance.test.trim().is_empty());
        assert!(!scenario.provenance.license.trim().is_empty());
        assert!(!scenario.sequenced_inputs.is_empty());
        assert!(scenario.initial_workspace.is_object());
        assert!(scenario.expected_outcome.is_object());
        assert!(scenario.expected_canonical_workspace.is_object());
    }

    let encoded = serde_json::to_string(&fixture).expect("fixture serialization must succeed");
    let decoded = decode_fixture(&encoded).expect("canonical fixture must decode");
    assert_eq!(decoded, fixture);
}

#[test]
fn unknown_fixture_version_is_rejected() {
    let mut value: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/docking_contract_v1.json"))
            .expect("the repository-owned fixture must be valid JSON");
    value["schema_version"] = serde_json::json!(dockspace::CONTRACT_VERSION + 1);

    let encoded = serde_json::to_string(&value).expect("fixture serialization must succeed");
    assert!(decode_fixture(&encoded).is_err());
}
