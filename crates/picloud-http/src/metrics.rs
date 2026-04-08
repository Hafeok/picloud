/// Platform Metrics Agent (ADR-040)
///
/// Collects hardware metrics from /proc and /sys on a configurable interval
/// and emits MetricRecorded events to the platform event log. Runs as a
/// background tokio task inside the picloud-server binary.

use std::sync::Arc;

use tracing::{debug, error, warn};
use uuid::Uuid;

use picloud_domain::events::{EventEnvelope, MetricEntry};
use picloud_domain::iri::{IriBuilder, ResourceIri};
use picloud_domain::traits::EventLog;

/// The metrics agent collects hardware telemetry and emits MetricRecorded events.
pub struct MetricsAgent {
    event_log: Arc<dyn EventLog>,
    iri_builder: IriBuilder,
    node_iri: ResourceIri,
}

impl MetricsAgent {
    pub fn new(
        event_log: Arc<dyn EventLog>,
        iri_builder: IriBuilder,
        node_iri: ResourceIri,
    ) -> Self {
        Self {
            event_log,
            iri_builder,
            node_iri,
        }
    }

    /// Start the metrics collection loop as a background task.
    /// Returns a JoinHandle for the spawned task.
    pub fn start(self, interval_secs: u64) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let interval = tokio::time::Duration::from_secs(interval_secs);
            tracing::info!(
                interval_secs = interval_secs,
                node_iri = %self.node_iri,
                "Metrics agent started"
            );

            loop {
                tokio::time::sleep(interval).await;

                let metrics = collect_system_metrics();
                if metrics.is_empty() {
                    warn!("No metrics collected — skipping emission");
                    continue;
                }

                let payload = serde_json::json!({
                    "node_iri": self.node_iri.as_str(),
                    "metrics": metrics,
                });

                let event = EventEnvelope::new(
                    self.iri_builder.event_schema("MetricRecorded", 1),
                    "MetricRecorded",
                    self.node_iri.clone(),
                    None, // platform-level event, no product scope
                    Uuid::new_v4(),
                    payload,
                );

                if let Err(e) = self.event_log.append(event).await {
                    error!("Failed to emit MetricRecorded event: {e}");
                } else {
                    debug!(node_iri = %self.node_iri, "MetricRecorded event emitted");
                }
            }
        })
    }
}

/// Collect system metrics from /proc and /sys.
/// Returns an empty vec if nothing could be read (e.g. on non-Linux).
fn collect_system_metrics() -> Vec<MetricEntry> {
    let mut metrics = Vec::new();

    if let Some(cpu) = read_cpu_usage() {
        metrics.push(MetricEntry {
            name: "cpu_usage_percent".to_string(),
            value: cpu,
            unit: "percent".to_string(),
        });
    }

    if let Some((used, total)) = read_memory_info() {
        metrics.push(MetricEntry {
            name: "memory_used_mb".to_string(),
            value: used,
            unit: "mb".to_string(),
        });
        metrics.push(MetricEntry {
            name: "memory_total_mb".to_string(),
            value: total,
            unit: "mb".to_string(),
        });
    }

    if let Some((used, total)) = read_disk_usage() {
        metrics.push(MetricEntry {
            name: "disk_used_gb".to_string(),
            value: used,
            unit: "gb".to_string(),
        });
        metrics.push(MetricEntry {
            name: "disk_total_gb".to_string(),
            value: total,
            unit: "gb".to_string(),
        });
    }

    if let Some(temp) = read_cpu_temperature() {
        metrics.push(MetricEntry {
            name: "cpu_temp_celsius".to_string(),
            value: temp,
            unit: "celsius".to_string(),
        });
    }

    metrics
}

// ---------------------------------------------------------------------------
// /proc and /sys parsers
// ---------------------------------------------------------------------------

/// Read CPU usage percentage from /proc/stat.
///
/// Takes two snapshots 100ms apart and computes the delta. This gives an
/// instantaneous-ish CPU utilisation figure rather than a cumulative one.
fn read_cpu_usage() -> Option<f64> {
    let first = parse_proc_stat_cpu(&std::fs::read_to_string("/proc/stat").ok()?)?;
    std::thread::sleep(std::time::Duration::from_millis(100));
    let second = parse_proc_stat_cpu(&std::fs::read_to_string("/proc/stat").ok()?)?;

    let idle_delta = second.idle - first.idle;
    let total_delta = second.total - first.total;

    if total_delta == 0 {
        return Some(0.0);
    }

    let usage = 100.0 * (1.0 - (idle_delta as f64 / total_delta as f64));
    Some((usage * 10.0).round() / 10.0) // 1 decimal place
}

struct CpuTimes {
    idle: u64,
    total: u64,
}

/// Parse the aggregate "cpu" line from /proc/stat content.
fn parse_proc_stat_cpu(content: &str) -> Option<CpuTimes> {
    // The first line looks like: cpu  user nice system idle iowait irq softirq steal ...
    let line = content.lines().find(|l| l.starts_with("cpu "))?;
    let fields: Vec<u64> = line
        .split_whitespace()
        .skip(1) // skip "cpu"
        .filter_map(|f| f.parse().ok())
        .collect();

    if fields.len() < 4 {
        return None;
    }

    let idle = fields[3]; // idle is the 4th field
    let iowait = fields.get(4).copied().unwrap_or(0);
    let total: u64 = fields.iter().sum();

    Some(CpuTimes {
        idle: idle + iowait,
        total,
    })
}

/// Read memory info from /proc/meminfo.
/// Returns (used_mb, total_mb).
fn read_memory_info() -> Option<(f64, f64)> {
    let content = std::fs::read_to_string("/proc/meminfo").ok()?;
    parse_meminfo(&content)
}

/// Parse /proc/meminfo content. Returns (used_mb, total_mb).
pub(crate) fn parse_meminfo(content: &str) -> Option<(f64, f64)> {
    let mut total_kb: Option<u64> = None;
    let mut available_kb: Option<u64> = None;

    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            total_kb = rest.trim().strip_suffix("kB").and_then(|s| s.trim().parse().ok());
        } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
            available_kb = rest.trim().strip_suffix("kB").and_then(|s| s.trim().parse().ok());
        }
        if total_kb.is_some() && available_kb.is_some() {
            break;
        }
    }

    let total = total_kb? as f64 / 1024.0;
    let available = available_kb? as f64 / 1024.0;
    let used = total - available;

    Some((
        (used * 10.0).round() / 10.0,
        (total * 10.0).round() / 10.0,
    ))
}

/// Read disk usage using statvfs on "/".
/// Returns (used_gb, total_gb).
fn read_disk_usage() -> Option<(f64, f64)> {
    // Use libc::statvfs to get filesystem stats
    #[cfg(target_os = "linux")]
    {
        use std::ffi::CString;
        use std::mem::MaybeUninit;

        let path = CString::new("/").ok()?;
        let mut stat = MaybeUninit::<libc::statvfs>::uninit();
        let ret = unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) };
        if ret != 0 {
            return None;
        }
        let stat = unsafe { stat.assume_init() };

        let block_size = stat.f_frsize as f64;
        let total_gb = (stat.f_blocks as f64 * block_size) / (1024.0 * 1024.0 * 1024.0);
        let free_gb = (stat.f_bavail as f64 * block_size) / (1024.0 * 1024.0 * 1024.0);
        let used_gb = total_gb - free_gb;

        Some((
            (used_gb * 10.0).round() / 10.0,
            (total_gb * 10.0).round() / 10.0,
        ))
    }

    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// Read CPU temperature from /sys/class/thermal/thermal_zone0/temp.
/// The file contains millidegrees Celsius as an integer (e.g. 58100 = 58.1C).
fn read_cpu_temperature() -> Option<f64> {
    let content = std::fs::read_to_string("/sys/class/thermal/thermal_zone0/temp").ok()?;
    parse_thermal_zone(&content)
}

/// Parse thermal zone content (millidegrees integer) into Celsius.
pub(crate) fn parse_thermal_zone(content: &str) -> Option<f64> {
    let millidegrees: f64 = content.trim().parse().ok()?;
    let celsius = millidegrees / 1000.0;
    Some((celsius * 10.0).round() / 10.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_proc_stat_cpu_basic() {
        let content = "cpu  10132 391 3026 6754430 3288 0 260 0 0 0\ncpu0 1234 10 300 1000000 100 0 30 0 0 0\n";
        let times = parse_proc_stat_cpu(content).unwrap();
        // idle = 6754430, iowait = 3288
        assert_eq!(times.idle, 6754430 + 3288);
        // total = sum of all fields
        let expected_total: u64 = 10132 + 391 + 3026 + 6754430 + 3288 + 0 + 260 + 0 + 0 + 0;
        assert_eq!(times.total, expected_total);
    }

    #[test]
    fn parse_proc_stat_cpu_missing_returns_none() {
        let content = "bogus data";
        assert!(parse_proc_stat_cpu(content).is_none());
    }

    #[test]
    fn parse_meminfo_basic() {
        let content = "\
MemTotal:       16384000 kB
MemFree:         2000000 kB
MemAvailable:    8192000 kB
Buffers:          500000 kB
";
        let (used, total) = parse_meminfo(content).unwrap();
        // total = 16384000 / 1024 = 16000.0 MB
        assert!((total - 16000.0).abs() < 0.2);
        // available = 8192000 / 1024 = 8000.0 MB
        // used = 16000 - 8000 = 8000.0 MB
        assert!((used - 8000.0).abs() < 0.2);
    }

    #[test]
    fn parse_meminfo_missing_field() {
        let content = "MemTotal:       16384000 kB\n";
        // Missing MemAvailable -> None
        assert!(parse_meminfo(content).is_none());
    }

    #[test]
    fn parse_thermal_zone_basic() {
        assert_eq!(parse_thermal_zone("58100\n"), Some(58.1));
        assert_eq!(parse_thermal_zone("42000\n"), Some(42.0));
        assert_eq!(parse_thermal_zone("72500"), Some(72.5));
    }

    #[test]
    fn parse_thermal_zone_invalid() {
        assert!(parse_thermal_zone("not_a_number\n").is_none());
        assert!(parse_thermal_zone("").is_none());
    }

    #[test]
    fn metric_entry_serializes() {
        let entry = MetricEntry {
            name: "cpu_usage_percent".to_string(),
            value: 42.3,
            unit: "percent".to_string(),
        };
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["name"], "cpu_usage_percent");
        assert_eq!(json["value"], 42.3);
        assert_eq!(json["unit"], "percent");
    }
}
