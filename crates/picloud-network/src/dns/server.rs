//! Authoritative DNS server — listens on UDP/TCP port 53.
//!
//! Uses the DnsResolver for query processing. Spawns background tasks
//! for UDP/TCP listeners and periodic cache eviction.

use std::sync::Arc;
use std::time::Duration;

use hickory_proto::op::{Header, MessageType, OpCode, ResponseCode};
use hickory_proto::rr::{DNSClass, Name, RData, Record, RecordType};
use hickory_proto::serialize::binary::{BinDecodable, BinEncodable};
use tokio::net::UdpSocket;
use tracing::{debug, error, info, warn};

use super::cache::{DnsRecord, SharedDnsCache};
use super::resolver::{DnsResolver, ResolveResult};

/// Configuration for the DNS server.
pub struct DnsServerConfig {
    /// Address to bind to (e.g., "0.0.0.0:53").
    pub bind_addr: String,
    /// The tenant domain this server is authoritative for.
    pub domain: String,
    /// Interval in seconds for cache eviction background task.
    pub evict_interval_secs: u64,
}

impl Default for DnsServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0:53".to_string(),
            domain: "picloud.local".to_string(),
            evict_interval_secs: 60,
        }
    }
}

/// Start the authoritative DNS server.
///
/// Spawns:
/// - UDP listener on the configured bind address
/// - Periodic cache eviction task
///
/// Note: Port 53 requires CAP_NET_BIND_SERVICE on Linux.
pub async fn start(
    config: DnsServerConfig,
    resolver: Arc<DnsResolver>,
    cache: SharedDnsCache,
) -> picloud_domain::error::Result<()> {
    info!(
        bind = %config.bind_addr,
        domain = %config.domain,
        "Starting authoritative DNS server"
    );

    // Spawn cache eviction background task
    let evict_cache = cache.clone();
    let evict_interval = config.evict_interval_secs;
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(evict_interval));
        loop {
            interval.tick().await;
            let mut c = evict_cache.write().await;
            c.evict_expired();
        }
    });

    // Spawn UDP listener with hickory-proto wire format parsing
    let bind_addr = config.bind_addr.clone();
    tokio::spawn(async move {
        match UdpSocket::bind(&bind_addr).await {
            Ok(socket) => {
                info!(addr = %bind_addr, "DNS UDP socket bound");
                let mut buf = [0u8; 512];
                loop {
                    match socket.recv_from(&mut buf).await {
                        Ok((len, src)) => {
                            // Parse DNS wire format
                            let request = match hickory_proto::op::Message::from_bytes(&buf[..len]) {
                                Ok(msg) => msg,
                                Err(e) => {
                                    warn!(src = %src, error = %e, "Failed to parse DNS query");
                                    continue;
                                }
                            };

                            let response = handle_dns_query(&request, &resolver).await;

                            match response.to_bytes() {
                                Ok(response_bytes) => {
                                    if let Err(e) = socket.send_to(&response_bytes, &src).await {
                                        warn!(src = %src, error = %e, "Failed to send DNS response");
                                    }
                                }
                                Err(e) => {
                                    warn!(error = %e, "Failed to serialize DNS response");
                                }
                            }
                        }
                        Err(e) => {
                            error!(error = %e, "DNS UDP recv error");
                        }
                    }
                }
            }
            Err(e) => {
                error!(
                    addr = %bind_addr,
                    error = %e,
                    "Failed to bind DNS UDP socket (requires CAP_NET_BIND_SERVICE for port 53)"
                );
            }
        }
    });

    Ok(())
}

/// Process a parsed DNS query and return a response message.
async fn handle_dns_query(
    request: &hickory_proto::op::Message,
    resolver: &Arc<DnsResolver>,
) -> hickory_proto::op::Message {
    let mut response = hickory_proto::op::Message::new();

    let mut header = Header::response_from_request(request.header());
    header.set_op_code(OpCode::Query);
    header.set_message_type(MessageType::Response);
    header.set_authoritative(true);
    response.set_header(header);

    // Copy the question section into the response
    for query in request.queries() {
        response.add_query(query.clone());
    }

    // Process each question
    for query in request.queries() {
        let name = query.name().to_string();
        // Remove trailing dot from DNS name
        let name = name.trim_end_matches('.');
        let rtype = match query.query_type() {
            RecordType::A => "A",
            RecordType::SRV => "SRV",
            RecordType::TXT => "TXT",
            RecordType::PTR => "PTR",
            _ => {
                debug!(name = %name, qtype = ?query.query_type(), "Unsupported record type");
                response.set_response_code(ResponseCode::NotImp);
                return response;
            }
        };

        match resolver.resolve(name, rtype).await {
            ResolveResult::Records { records, ttl } => {
                for record in records {
                    if let Some(rr) = dns_record_to_rr(name, &record, ttl) {
                        response.add_answer(rr);
                    }
                }
            }
            ResolveResult::NxDomain => {
                response.set_response_code(ResponseCode::NXDomain);
            }
            ResolveResult::NotImplemented => {
                response.set_response_code(ResponseCode::NotImp);
            }
        }
    }

    response
}

/// Convert a platform DnsRecord into a hickory-proto Record.
fn dns_record_to_rr(name: &str, record: &DnsRecord, ttl: u32) -> Option<Record> {
    let dns_name = Name::from_ascii(name).ok()?;
    match record {
        DnsRecord::A { address } => {
            let mut rr = Record::from_rdata(dns_name, ttl, RData::A((*address).into()));
            rr.set_dns_class(DNSClass::IN);
            Some(rr)
        }
        DnsRecord::Srv { target, port, priority, weight } => {
            let target_name = Name::from_ascii(target).ok()?;
            let srv = hickory_proto::rr::rdata::SRV::new(*priority, *weight, *port, target_name);
            let mut rr = Record::from_rdata(dns_name, ttl, RData::SRV(srv));
            rr.set_dns_class(DNSClass::IN);
            Some(rr)
        }
        DnsRecord::Txt { text } => {
            let txt = hickory_proto::rr::rdata::TXT::new(vec![text.clone()]);
            let mut rr = Record::from_rdata(dns_name, ttl, RData::TXT(txt));
            rr.set_dns_class(DNSClass::IN);
            Some(rr)
        }
        DnsRecord::Ptr { name: ptr_name } => {
            let ptr = Name::from_ascii(ptr_name).ok()?;
            let mut rr = Record::from_rdata(dns_name, ttl, RData::PTR(hickory_proto::rr::rdata::PTR(ptr)));
            rr.set_dns_class(DNSClass::IN);
            Some(rr)
        }
    }
}

/// Return a Pi-hole conditional forwarding configuration hint.
pub fn pihole_config_hint(domain: &str, node_ip: &str) -> String {
    format!(
        "Pi-hole conditional forwarding:\n  Domain: {domain}\n  DNS Server: {node_ip}"
    )
}
