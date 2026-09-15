//! `lapidary://open?part=<part id>`: the one link the desktop handler acts on (Phase 4 slice 2
//! spec §3).
//!
//! Any web page can hand the handler a link, so this reads one by hand and accepts exactly one
//! shape. Nothing a link carries can name a server, a folder, a file or a program: the server
//! and the workspace were fixed when the handler was registered, and a part id is all that is
//! left for a link to choose.

use lapidary_core::PartId;

const PREFIX: &str = "lapidary://open?part=";

/// The part a link asks to open, or what is wrong with the link, in words for the person who
/// clicked it.
pub fn part(link: &str) -> Result<PartId, String> {
    let Some(id) = link.strip_prefix(PREFIX) else {
        return Err(format!(
            "{link} is not a link Lapidary opens, so nothing was opened. The only one is \
             lapidary://open?part=<part id>, from a part's page."
        ));
    };
    // A part id is hex digits and hyphens. Anything else — a second pair, a fragment,
    // percent-encoding, a path — is a link carrying more than a part.
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return Err(format!(
            "{link} carries something other than a part id, so nothing was opened. Open the part \
             from its page in Lapidary."
        ));
    }
    id.parse().map_err(|_| {
        format!(
            "{link} does not name a part ({id} is not a part id), so nothing was opened. Open the \
             part from its page in Lapidary."
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "01931b6e-0000-7000-8000-00000000aaaa";

    #[test]
    fn a_part_link_names_its_part() {
        assert_eq!(
            part(&format!("lapidary://open?part={ID}")).map(|part| part.to_string()),
            Ok(ID.to_owned())
        );
    }

    #[test]
    fn a_link_carrying_anything_but_one_part_id_opens_nothing() {
        for link in [
            format!("https://example.com/open?part={ID}"),
            format!("lapidary://download?part={ID}"),
            format!("lapidary://open/x?part={ID}"),
            format!("lapidary://open:8080?part={ID}"),
            format!("lapidary://mira@open?part={ID}"),
            format!("lapidary://open?id={ID}"),
            format!("lapidary://open?part={ID}&server=http://attacker.example"),
            format!("lapidary://open?part={ID}&part={ID}"),
            format!("lapidary://open?part={ID}#details"),
            "lapidary://open?part=01931b6e-0000-7000-8000-00000000aa%61a".to_owned(),
            "lapidary://open?part=../../.ssh/id_ed25519".to_owned(),
            "lapidary://open?part=".to_owned(),
            "lapidary://open?part=abc".to_owned(),
        ] {
            assert!(part(&link).is_err(), "{link} must open nothing");
        }
    }
}
