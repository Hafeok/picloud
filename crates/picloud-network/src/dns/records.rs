//! SPARQL query templates and record construction for DNS resolution.
//!
//! Each record type (A, SRV, TXT, PTR) has a corresponding SPARQL query
//! that extracts the necessary data from the RDF graph.

/// SPARQL query to resolve A records.
/// Checks ingress hostnames, node addresses, and product hostnames.
pub const A_RECORD_QUERY: &str = r#"
PREFIX pc: <https://picloud.local/ontology#>
PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
SELECT ?address WHERE {
    {
        # Ingress hostname -> workload node address
        ?ingress a pc:Ingress ;
                 pc:hostname ?hostname ;
                 pc:ingressAddress ?address .
        FILTER(?hostname = $hostname)
    } UNION {
        # Node hostname -> node address
        ?node a pc:Node ;
              pc:hostname ?hostname ;
              pc:nodeAddress ?address .
        FILTER(?hostname = $hostname)
    } UNION {
        # Product hostname -> product endpoint
        ?product a pc:Product ;
                 pc:hostname ?hostname ;
                 pc:nodeAddress ?address .
        FILTER(?hostname = $hostname)
    }
}
"#;

/// SPARQL query to resolve SRV records.
/// Used for service discovery (e.g., _sparql._tcp.photo-app.picloud.local).
pub const SRV_RECORD_QUERY: &str = r#"
PREFIX pc: <https://picloud.local/ontology#>
SELECT ?target ?port ?priority ?weight WHERE {
    ?service a pc:Service ;
             pc:serviceType ?svc_type ;
             pc:product ?product ;
             pc:target ?target ;
             pc:port ?port .
    OPTIONAL { ?service pc:priority ?priority }
    OPTIONAL { ?service pc:weight ?weight }
    FILTER(?svc_type = $service_type && ?product = $product)
}
"#;

/// SPARQL query to resolve TXT records.
/// Returns semantic metadata: ontology IRI, version, cluster info.
pub const TXT_RECORD_QUERY: &str = r#"
PREFIX pc: <https://picloud.local/ontology#>
SELECT ?key ?value WHERE {
    {
        ?product a pc:Product ;
                 pc:hostname ?hostname ;
                 pc:version ?value .
        BIND("version" AS ?key)
        FILTER(?hostname = $hostname)
    } UNION {
        ?product a pc:Product ;
                 pc:hostname ?hostname ;
                 pc:ontologyIri ?value .
        BIND("ontology" AS ?key)
        FILTER(?hostname = $hostname)
    }
}
"#;

/// SPARQL query to resolve PTR records (reverse DNS).
pub const PTR_RECORD_QUERY: &str = r#"
PREFIX pc: <https://picloud.local/ontology#>
SELECT ?hostname WHERE {
    {
        ?node a pc:Node ;
              pc:nodeAddress ?address ;
              pc:hostname ?hostname .
        FILTER(?address = $address)
    } UNION {
        ?ingress a pc:Ingress ;
                 pc:ingressAddress ?address ;
                 pc:hostname ?hostname .
        FILTER(?address = $address)
    }
}
"#;

/// Parse an SRV query name into its components.
///
/// Format: `_service._proto.product.domain`
/// Example: `_sparql._tcp.photo-app.picloud.local` -> ("sparql", "tcp", "photo-app")
pub fn parse_srv_name(name: &str, domain: &str) -> Option<(String, String, String)> {
    let suffix = format!(".{domain}");
    let without_domain = name.strip_suffix(&suffix)?;

    let parts: Vec<&str> = without_domain.splitn(3, '.').collect();
    if parts.len() != 3 {
        return None;
    }

    let service = parts[0].strip_prefix('_')?;
    let proto = parts[1].strip_prefix('_')?;
    let product = parts[2];

    Some((service.to_string(), proto.to_string(), product.to_string()))
}

/// Parse a PTR query name (reverse DNS) into an IPv4 address string.
///
/// Format: `d.c.b.a.in-addr.arpa` -> "a.b.c.d"
pub fn parse_ptr_name(name: &str) -> Option<String> {
    let without_suffix = name.strip_suffix(".in-addr.arpa")?;
    let octets: Vec<&str> = without_suffix.split('.').collect();
    if octets.len() != 4 {
        return None;
    }
    // Reverse the octets
    Some(format!("{}.{}.{}.{}", octets[3], octets[2], octets[1], octets[0]))
}

/// Format a TXT record for a product.
pub fn format_product_txt(key: &str, value: &str) -> String {
    format!("{key}={value}")
}

/// Format a TXT record for the cluster.
pub fn format_cluster_txt(cluster_id: &str, version: &str) -> String {
    format!("cluster_id={cluster_id} version={version}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_srv_name_valid() {
        let result = parse_srv_name("_sparql._tcp.photo-app.picloud.local", "picloud.local");
        assert_eq!(result, Some(("sparql".to_string(), "tcp".to_string(), "photo-app".to_string())));
    }

    #[test]
    fn parse_srv_name_invalid_no_underscore() {
        assert!(parse_srv_name("sparql.tcp.photo-app.picloud.local", "picloud.local").is_none());
    }

    #[test]
    fn parse_srv_name_wrong_domain() {
        assert!(parse_srv_name("_sparql._tcp.photo-app.other.local", "picloud.local").is_none());
    }

    #[test]
    fn parse_ptr_name_valid() {
        let result = parse_ptr_name("10.1.168.192.in-addr.arpa");
        assert_eq!(result, Some("192.168.1.10".to_string()));
    }

    #[test]
    fn parse_ptr_name_invalid() {
        assert!(parse_ptr_name("10.1.168.in-addr.arpa").is_none());
    }

    #[test]
    fn parse_ptr_name_not_arpa() {
        assert!(parse_ptr_name("10.1.168.192.some.other").is_none());
    }

    #[test]
    fn format_product_txt_works() {
        assert_eq!(format_product_txt("version", "1.2.3"), "version=1.2.3");
    }
}
