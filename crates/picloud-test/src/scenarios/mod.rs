mod adr_test_coverage_completeness;
mod no_internal_imports;
mod scenario_catalogue_sync;
mod test_crate_builds_independently;
mod upgrade_compatibility;
mod rolling_upgrade_sequence;
mod upgrade_gate_enforcement;
mod shared_hardware_isolation;
mod port_non_conflict;
mod version_attribute_present;
mod version_matches_cluster;

use crate::harness::runner::Scenario;

/// Return all registered scenarios.
pub fn all_scenarios() -> Vec<Box<dyn Scenario>> {
    vec![
        // ADR-054: Test-augmented ADR template
        Box::new(adr_test_coverage_completeness::AdrTestCoverageCompleteness),
        Box::new(scenario_catalogue_sync::ScenarioCatalogueSync),
        // ADR-058: picloud-test as first-class crate
        Box::new(test_crate_builds_independently::TestCrateBuildsIndependently),
        Box::new(no_internal_imports::NoInternalImports),
        // ADR-055: Staged platform upgrade
        Box::new(upgrade_compatibility::UpgradeCompatibilityScenario),
        Box::new(rolling_upgrade_sequence::RollingUpgradeSequenceScenario),
        Box::new(upgrade_gate_enforcement::UpgradeGateEnforcementScenario),
        // ADR-056: Shared hardware staging
        Box::new(shared_hardware_isolation::SharedHardwareIsolationScenario),
        Box::new(port_non_conflict::PortNonConflictScenario),
        // ADR-057: OTel test data versioning
        Box::new(version_attribute_present::VersionAttributePresentScenario),
        Box::new(version_matches_cluster::VersionMatchesClusterScenario),
    ]
}
