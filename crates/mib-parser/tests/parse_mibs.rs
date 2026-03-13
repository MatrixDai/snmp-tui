use std::path::PathBuf;

use mib_parser::parser::parse_mib_raw;
use mib_parser::{Oid, load_mibs};

fn mibs_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("mibs")
}

#[test]
fn parse_snmpv2_smi() {
    let source = std::fs::read_to_string(mibs_dir().join("SNMPv2-SMI.txt")).unwrap();
    let modules = parse_mib_raw(&source).expect("Failed to parse SNMPv2-SMI");
    assert_eq!(modules.len(), 1);
    assert_eq!(modules[0].name, "SNMPv2-SMI");
    assert!(modules[0].oid_assignments.contains_key("org"));
    assert!(modules[0].oid_assignments.contains_key("internet"));
    assert!(modules[0].oid_assignments.contains_key("enterprises"));
}

#[test]
fn parse_snmpv2_tc() {
    let source = std::fs::read_to_string(mibs_dir().join("SNMPv2-TC.txt")).unwrap();
    let modules = parse_mib_raw(&source).expect("Failed to parse SNMPv2-TC");
    assert_eq!(modules.len(), 1);
    assert_eq!(modules[0].name, "SNMPv2-TC");
}

#[test]
fn parse_snmpv2_conf() {
    let source = std::fs::read_to_string(mibs_dir().join("SNMPv2-CONF.txt")).unwrap();
    let modules = parse_mib_raw(&source).expect("Failed to parse SNMPv2-CONF");
    assert_eq!(modules.len(), 1);
    assert_eq!(modules[0].name, "SNMPv2-CONF");
}

#[test]
fn parse_snmpv2_mib() {
    let source = std::fs::read_to_string(mibs_dir().join("SNMPv2-MIB.txt")).unwrap();
    let modules = parse_mib_raw(&source).expect("Failed to parse SNMPv2-MIB");
    assert_eq!(modules.len(), 1);
    assert_eq!(modules[0].name, "SNMPv2-MIB");
}

#[test]
fn parse_if_mib() {
    let source = std::fs::read_to_string(mibs_dir().join("IF-MIB.txt")).unwrap();
    let modules = parse_mib_raw(&source).expect("Failed to parse IF-MIB");
    assert_eq!(modules.len(), 1);
    assert_eq!(modules[0].name, "IF-MIB");
}

#[test]
fn parse_ip_mib() {
    let source = std::fs::read_to_string(mibs_dir().join("IP-MIB.txt")).unwrap();
    let modules = parse_mib_raw(&source).expect("Failed to parse IP-MIB");
    assert_eq!(modules.len(), 1);
    assert_eq!(modules[0].name, "IP-MIB");
}

#[test]
fn parse_tcp_mib() {
    let source = std::fs::read_to_string(mibs_dir().join("TCP-MIB.txt")).unwrap();
    let modules = parse_mib_raw(&source).expect("Failed to parse TCP-MIB");
    assert_eq!(modules.len(), 1);
    assert_eq!(modules[0].name, "TCP-MIB");
}

#[test]
fn parse_udp_mib() {
    let source = std::fs::read_to_string(mibs_dir().join("UDP-MIB.txt")).unwrap();
    let modules = parse_mib_raw(&source).expect("Failed to parse UDP-MIB");
    assert_eq!(modules.len(), 1);
    assert_eq!(modules[0].name, "UDP-MIB");
}

#[test]
fn parse_host_resources_mib() {
    let source = std::fs::read_to_string(mibs_dir().join("HOST-RESOURCES-MIB.txt")).unwrap();
    let modules = parse_mib_raw(&source).expect("Failed to parse HOST-RESOURCES-MIB");
    assert_eq!(modules.len(), 1);
    assert_eq!(modules[0].name, "HOST-RESOURCES-MIB");
}

#[test]
fn load_core_mibs_and_verify_oids() {
    let paths: Vec<PathBuf> = [
        "SNMPv2-SMI.txt",
        "SNMPv2-TC.txt",
        "SNMPv2-CONF.txt",
        "SNMPv2-MIB.txt",
        "IF-MIB.txt",
        "IANAifType-MIB.txt",
    ]
    .iter()
    .map(|f| mibs_dir().join(f))
    .collect();

    let tree = load_mibs(&paths).expect("Failed to load MIBs");

    // sysDescr = 1.3.6.1.2.1.1.1
    let sys_descr_oid = Oid::new(vec![1, 3, 6, 1, 2, 1, 1, 1]);
    let idx = tree.lookup(&sys_descr_oid);
    assert!(
        idx.is_some(),
        "sysDescr should be in tree at 1.3.6.1.2.1.1.1"
    );
    let node = tree.get(idx.unwrap()).unwrap();
    assert_eq!(node.name, "sysDescr");

    // sysObjectID = 1.3.6.1.2.1.1.2
    let sys_object_id_oid = Oid::new(vec![1, 3, 6, 1, 2, 1, 1, 2]);
    let idx = tree.lookup(&sys_object_id_oid);
    assert!(idx.is_some(), "sysObjectID should be in tree");
    assert_eq!(tree.get(idx.unwrap()).unwrap().name, "sysObjectID");

    // sysUpTime = 1.3.6.1.2.1.1.3
    let sys_uptime_oid = Oid::new(vec![1, 3, 6, 1, 2, 1, 1, 3]);
    let idx = tree.lookup(&sys_uptime_oid);
    assert!(idx.is_some(), "sysUpTime should be in tree");
    assert_eq!(tree.get(idx.unwrap()).unwrap().name, "sysUpTime");

    // ifTable = 1.3.6.1.2.1.2.2
    let if_table_oid = Oid::new(vec![1, 3, 6, 1, 2, 1, 2, 2]);
    let idx = tree.lookup(&if_table_oid);
    assert!(idx.is_some(), "ifTable should be in tree");
    assert_eq!(tree.get(idx.unwrap()).unwrap().name, "ifTable");

    // Verify MIB object metadata on sysDescr
    let sys_descr_node = tree.get(tree.lookup(&sys_descr_oid).unwrap()).unwrap();
    let mib_obj = sys_descr_node
        .mib_object
        .as_ref()
        .expect("sysDescr should have MibObject");
    assert_eq!(mib_obj.module, "SNMPv2-MIB");
    assert!(mib_obj.description.is_some());
}

#[test]
fn load_snmpv2_smi_to_mib_import_chain() {
    let paths: Vec<PathBuf> = [
        "SNMPv2-SMI.txt",
        "SNMPv2-TC.txt",
        "SNMPv2-CONF.txt",
        "SNMPv2-MIB.txt",
    ]
    .iter()
    .map(|f| mibs_dir().join(f))
    .collect();

    let tree = load_mibs(&paths).expect("Failed to load MIBs");

    // snmpMIB = 1.3.6.1.6.3.1
    let snmp_mib_oid = Oid::new(vec![1, 3, 6, 1, 6, 3, 1]);
    let idx = tree.lookup(&snmp_mib_oid);
    assert!(idx.is_some(), "snmpMIB should be in tree at 1.3.6.1.6.3.1");

    // Tree should have many nodes
    assert!(
        tree.len() > 20,
        "Tree should have many nodes, got {}",
        tree.len()
    );
}

#[test]
fn load_all_bundled_mibs() {
    let paths: Vec<PathBuf> = [
        "SNMPv2-SMI.txt",
        "SNMPv2-TC.txt",
        "SNMPv2-CONF.txt",
        "SNMPv2-MIB.txt",
        "IF-MIB.txt",
        "IANAifType-MIB.txt",
        "IP-MIB.txt",
        "IP-FORWARD-MIB.txt",
        "TCP-MIB.txt",
        "UDP-MIB.txt",
        "HOST-RESOURCES-MIB.txt",
        "HOST-RESOURCES-TYPES.txt",
        "SNMP-FRAMEWORK-MIB.txt",
        "IANA-RTPROTO-MIB.txt",
    ]
    .iter()
    .map(|f| mibs_dir().join(f))
    .collect();

    let tree = load_mibs(&paths).expect("Failed to load all MIBs");

    // Verify tree has substantial content
    assert!(
        tree.len() > 100,
        "Tree should have >100 nodes, got {}",
        tree.len()
    );

    // ifDescr (IF-MIB) = 1.3.6.1.2.1.2.2.1.2
    let if_descr = Oid::new(vec![1, 3, 6, 1, 2, 1, 2, 2, 1, 2]);
    assert!(
        tree.lookup(&if_descr).is_some(),
        "ifDescr should be in tree"
    );

    // tcpConnState (TCP-MIB) = 1.3.6.1.2.1.6.13.1.1
    let tcp_conn_state = Oid::new(vec![1, 3, 6, 1, 2, 1, 6, 13, 1, 1]);
    assert!(
        tree.lookup(&tcp_conn_state).is_some(),
        "tcpConnState should be in tree"
    );
}
