//! DataProductSLOMonitor — monitors data product freshness and emits
//! SLO breach/restore actions (ADR-056).
//!
//! Depends only on picloud-domain. Queries the RDF graph for data
//! products with lastRefreshed timestamps and maxAge declarations,
//! comparing against the current time.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::Utc;
use tracing::{debug, info, warn};

use picloud_domain::error::{PiCloudError, Result};
use picloud_domain::events::{DataProductSLOBreachedPayload, DataProductSLORestoredPayload};
use picloud_domain::iri::ResourceIri;
use picloud_domain::traits::{DataProductSLOAction, DataProductSLOMonitor, QueryResult, StateProjector};

/// SLO monitor that checks all data products for freshness breaches.
///
/// Uses the `StateProjector` to query the RDF graph for data product
/// metadata (lastRefreshed, maxAge). Tracks which data products are
/// currently in breach so it can emit Restore actions when they recover.
pub struct RdfDataProductSLOMonitor {
    projector: Arc<dyn StateProjector>,
    /// Set of data product IRIs currently in breach state.
    breached: Mutex<HashSet<String>>,
}

impl RdfDataProductSLOMonitor {
    /// Create a new SLO monitor backed by the given state projector.
    pub fn new(projector: Arc<dyn StateProjector>) -> Self {
        Self {
            projector,
            breached: Mutex::new(HashSet::new()),
        }
    }

    /// Parse an ISO 8601 duration string (e.g. "PT1H", "PT30M", "PT3600S")
    /// into seconds. Supports hours (H), minutes (M), and seconds (S).
    fn parse_max_age_seconds(max_age: &str) -> Option<u64> {
        let s = max_age.trim();
        if !s.starts_with("PT") && !s.starts_with("pt") {
            // Try parsing as raw seconds
            return s.parse::<u64>().ok();
        }
        let body = &s[2..];
        let mut total_seconds: u64 = 0;
        let mut num_buf = String::new();

        for ch in body.chars() {
            match ch {
                '0'..='9' | '.' => num_buf.push(ch),
                'H' | 'h' => {
                    let hours: f64 = num_buf.parse().ok()?;
                    total_seconds += (hours * 3600.0) as u64;
                    num_buf.clear();
                }
                'M' | 'm' => {
                    let minutes: f64 = num_buf.parse().ok()?;
                    total_seconds += (minutes * 60.0) as u64;
                    num_buf.clear();
                }
                'S' | 's' => {
                    let secs: f64 = num_buf.parse().ok()?;
                    total_seconds += secs as u64;
                    num_buf.clear();
                }
                _ => return None,
            }
        }

        if total_seconds > 0 {
            Some(total_seconds)
        } else {
            None
        }
    }
}

#[async_trait]
impl DataProductSLOMonitor for RdfDataProductSLOMonitor {
    async fn check_freshness(&self) -> Result<Vec<DataProductSLOAction>> {
        // Query all data products with their lastRefreshed and maxAge
        let sparql = r#"
            PREFIX pc: <https://picloud.local/ontology#>
            SELECT ?dp ?lastRefreshed ?maxAge WHERE {
                ?dp a pc:DataProduct .
                ?dp pc:lastRefreshed ?lastRefreshed .
                ?dp pc:maxAge ?maxAge .
            }
        "#;

        let result = self.projector.query(sparql).await?;
        let now = Utc::now();
        let mut actions = Vec::new();

        for binding in &result.bindings {
            let dp_iri_str = match binding.get("dp").and_then(|v| v.as_str()) {
                Some(s) => s.to_owned(),
                None => continue,
            };

            let last_refreshed_str = match binding.get("lastRefreshed").and_then(|v| v.as_str()) {
                Some(s) => s,
                None => continue,
            };

            let max_age_str = match binding.get("maxAge").and_then(|v| v.as_str()) {
                Some(s) => s,
                None => continue,
            };

            let last_refreshed = match chrono::DateTime::parse_from_rfc3339(last_refreshed_str) {
                Ok(dt) => dt.with_timezone(&Utc),
                Err(e) => {
                    warn!(
                        dp = %dp_iri_str,
                        error = %e,
                        "Failed to parse lastRefreshed timestamp"
                    );
                    continue;
                }
            };

            let max_age_secs = match Self::parse_max_age_seconds(max_age_str) {
                Some(s) => s,
                None => {
                    warn!(
                        dp = %dp_iri_str,
                        max_age = max_age_str,
                        "Failed to parse maxAge duration"
                    );
                    continue;
                }
            };

            let actual_age = now.signed_duration_since(last_refreshed);
            let actual_age_seconds = actual_age.num_seconds().max(0) as u64;

            let mut breached_set = self.breached.lock().map_err(|e| {
                PiCloudError::Internal(format!("breached set lock poisoned: {e}"))
            })?;

            if actual_age_seconds > max_age_secs {
                // SLO breached
                if breached_set.insert(dp_iri_str.clone()) {
                    // Newly breached (was not in the set before)
                    info!(
                        dp = %dp_iri_str,
                        max_age = max_age_str,
                        actual_age_seconds,
                        "Data product SLO breached"
                    );
                    actions.push(DataProductSLOAction::Breach(
                        DataProductSLOBreachedPayload {
                            data_product_iri: ResourceIri(dp_iri_str.clone()),
                            max_age: max_age_str.to_owned(),
                            actual_age_seconds,
                        },
                    ));
                } else {
                    debug!(
                        dp = %dp_iri_str,
                        actual_age_seconds,
                        "Data product still in SLO breach"
                    );
                }
            } else if breached_set.remove(&dp_iri_str) {
                // Was breached, now restored
                info!(
                    dp = %dp_iri_str,
                    "Data product SLO restored"
                );
                actions.push(DataProductSLOAction::Restore(
                    DataProductSLORestoredPayload {
                        data_product_iri: ResourceIri(dp_iri_str.clone()),
                        refreshed_at: last_refreshed,
                    },
                ));
            }
        }

        Ok(actions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_iso_duration_hours() {
        assert_eq!(RdfDataProductSLOMonitor::parse_max_age_seconds("PT1H"), Some(3600));
    }

    #[test]
    fn parse_iso_duration_minutes() {
        assert_eq!(RdfDataProductSLOMonitor::parse_max_age_seconds("PT30M"), Some(1800));
    }

    #[test]
    fn parse_iso_duration_seconds() {
        assert_eq!(RdfDataProductSLOMonitor::parse_max_age_seconds("PT3600S"), Some(3600));
    }

    #[test]
    fn parse_iso_duration_combined() {
        assert_eq!(
            RdfDataProductSLOMonitor::parse_max_age_seconds("PT1H30M"),
            Some(5400)
        );
    }

    #[test]
    fn parse_raw_seconds() {
        assert_eq!(RdfDataProductSLOMonitor::parse_max_age_seconds("300"), Some(300));
    }

    #[test]
    fn parse_invalid_returns_none() {
        assert_eq!(RdfDataProductSLOMonitor::parse_max_age_seconds("invalid"), None);
    }
}
