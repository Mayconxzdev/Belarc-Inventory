//! JSON Schema contracts for collectors (documentation + validation reference).

pub const COLLECTOR_NAMES: &[&str] = &[
    "identity",
    "os",
    "hardware",
    "network",
    "remote_access",
    "software",
    "licensing",
    "certificates",
    "security",
    "email",
    "peripherals",
    "permissions",
    "logins",
    "event_logs",
    "compliance",
];

pub const COLLECTOR_VERSION: &str = "1.0.0";
