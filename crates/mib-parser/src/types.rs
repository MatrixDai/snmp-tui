use crate::oid::Oid;

/// SNMP object access level (covers both SMIv1 and SMIv2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    NotAccessible,
    AccessibleForNotify,
    ReadOnly,
    ReadWrite,
    ReadCreate,
}

/// SNMP object status (covers both SMIv1 and SMIv2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Current,
    Deprecated,
    Obsolete,
    /// SMIv1 only
    Mandatory,
    /// SMIv1 only
    Optional,
}

/// MIB object syntax / type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Syntax {
    Integer,
    Integer32,
    OctetString,
    ObjectIdentifier,
    IpAddress,
    Counter32,
    Counter64,
    Gauge32,
    Unsigned32,
    TimeTicks,
    Opaque,
    Bits,
    /// Named textual convention (e.g. DisplayString, PhysAddress).
    TextualConvention(String),
    /// Integer enumeration: list of (label, value) pairs.
    IntegerEnum(Vec<(String, i64)>),
    /// Size/range constrained type.
    Constrained {
        base: Box<Syntax>,
        constraint: String,
    },
    /// SEQUENCE type used for table row definitions.
    Sequence(String),
}

/// A parsed MIB object definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MibObject {
    pub name: String,
    pub oid: Oid,
    pub module: String,
    pub syntax: Option<Syntax>,
    pub access: Option<Access>,
    pub status: Option<Status>,
    pub description: Option<String>,
    pub index_clause: Option<Vec<String>>,
    pub defval: Option<String>,
}
