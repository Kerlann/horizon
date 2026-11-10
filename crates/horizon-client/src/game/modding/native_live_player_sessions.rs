//! Heavy session strings from the actual record or the acting session lanes.
use super::{
    Game, commands::Target, native_base_read_rows::BaseReadSource,
    native_entity_body_provider::EntityBodyProvider, native_live_entity_reads::EntityReadAdmission,
};

pub(super) enum PlayerSessionField {
    Name,
    Address,
    Brand,
}
enum SessionValue<'a> {
    Text(&'a str),
    Address(std::net::IpAddr),
}

impl Game<'_> {
    /// Prefer a concrete lent record. The canonical record is a separate actual
    /// source part when the provider supplies only the acting motion body.
    pub(super) fn entity_read_guest<'a>(
        &'a self,
        source: BaseReadSource<'a>,
        target: Target,
    ) -> Option<&'a horizon_client::lan::guest::Guest> {
        if let BaseReadSource::Guest(guest) = source {
            return Some(guest);
        }
        let conn = match target {
            Target::Player => self.lan_acting?,
            Target::Guest(conn) => conn,
            _ => return None,
        };
        self.command_guest(conn)
    }

    pub(super) fn read_player_session_string(
        &self,
        admitted: EntityReadAdmission,
        provider: Option<&dyn EntityBodyProvider>,
        field: PlayerSessionField,
    ) -> Result<Option<String>, mod_api::EntityReadError> {
        self.with_entity_read_storage(admitted, provider, |source| {
            let target = admitted.target();
            let peer = self.entity_read_guest(source.body, target);
            if matches!(target, Target::Guest(_)) && peer.is_none() {
                return Err(mod_api::EntityReadError::Lifted);
            }
            let peer = peer.filter(|guest| guest.participates_online());
            let named_acting = matches!(target, Target::Player)
                && self.lan_acting_name.is_some()
                && self.lan_acting_permission.is_some();
            let value = match field {
                PlayerSessionField::Name
                    if matches!(target, Target::Player) && self.lan_acting_permission.is_some() =>
                {
                    self.lan_acting_name.as_deref().map(SessionValue::Text)
                }
                PlayerSessionField::Name => peer.map(|guest| SessionValue::Text(&guest.name)),
                PlayerSessionField::Address => peer
                    .map(|guest| SessionValue::Address(guest.address.ip()))
                    .or_else(|| named_acting.then_some(SessionValue::Text(""))),
                PlayerSessionField::Brand => peer
                    .map(|guest| SessionValue::Text(&guest.brand))
                    .or_else(|| named_acting.then_some(SessionValue::Text(""))),
            };
            let Some(value) = value else {
                return Ok(None);
            };
            admitted.project_actual(|| {
                Some(match value {
                    SessionValue::Text(text) => text.to_owned(),
                    SessionValue::Address(ip) => ip.to_string(),
                })
            })
        })
        .unwrap_or(Err(mod_api::EntityReadError::Lifted))
    }
}
