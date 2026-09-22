use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MachineAdmin {
    pub owner_name: Option<String>,
    pub ramal: Option<String>,
    pub primary_email: Option<String>,
    pub network_cable: Option<String>,
    pub cybersul_user: Option<String>,
    pub nas_user: Option<String>,
    pub notes: Option<String>,
    pub maintenance_status: Option<String>,
    pub maintenance_notes: Option<String>,
    pub ti_comments: Option<String>,
    /// Campos legados da primeira proposta de portal. Mantidos para migrar sem
    /// apagar dados, mas não são mais exibidos como configuração operacional.
    pub ticket_default_department: Option<String>,
    pub ticket_attendant_department: Option<String>,
    #[serde(default)]
    pub ticket_receive_departments: Vec<String>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateMachineAdmin {
    pub owner_name: Option<String>,
    pub ramal: Option<String>,
    pub primary_email: Option<String>,
    pub network_cable: Option<String>,
    pub cybersul_user: Option<String>,
    pub nas_user: Option<String>,
    pub notes: Option<String>,
    pub maintenance_status: Option<String>,
    pub maintenance_notes: Option<String>,
    pub ti_comments: Option<String>,
    pub ticket_default_department: Option<String>,
    pub ticket_attendant_department: Option<String>,
    pub ticket_receive_departments: Option<Vec<String>>,
}

impl UpdateMachineAdmin {
    pub fn into_record(self) -> MachineAdmin {
        MachineAdmin {
            owner_name: empty_to_none(self.owner_name),
            ramal: empty_to_none(self.ramal),
            primary_email: empty_to_none(self.primary_email),
            network_cable: empty_to_none(self.network_cable),
            cybersul_user: empty_to_none(self.cybersul_user),
            nas_user: empty_to_none(self.nas_user),
            notes: empty_to_none(self.notes),
            maintenance_status: empty_to_none(self.maintenance_status),
            maintenance_notes: empty_to_none(self.maintenance_notes),
            ti_comments: empty_to_none(self.ti_comments),
            ticket_default_department: empty_to_none(self.ticket_default_department),
            ticket_attendant_department: empty_to_none(self.ticket_attendant_department),
            ticket_receive_departments: self.ticket_receive_departments.unwrap_or_default(),
            updated_at: None,
        }
    }
}

fn empty_to_none(s: Option<String>) -> Option<String> {
    s.and_then(|v| {
        let t = v.trim().to_string();
        if t.is_empty() {
            None
        } else {
            Some(t)
        }
    })
}

pub fn display_email(admin: &MachineAdmin, thunderbird_emails: &[String]) -> Option<String> {
    admin
        .primary_email
        .clone()
        .or_else(|| thunderbird_emails.first().cloned())
}
