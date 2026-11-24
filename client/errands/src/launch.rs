//! What a launch opens on, from what the run before kept.

/// Every server added, with its account when one signed in there.
pub fn opened_at(rows: Vec<platform::state::Row>) -> Vec<screens::Opening> {
    rows.into_iter()
        .map(|row| {
            // The picture it last said is shown from this device's cache until it greets again, which its sign-in does.
            let server = screens::Server { url: row.server, name: row.name, colour: row.colour, picture: row.picture };
            match row.identity {
                Some(identity) => screens::Opening::Account(server, identity),
                None => screens::Opening::Server(server),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::opened_at;

    #[test]
    fn a_remembered_server_opens_with_the_picture_it_last_said() {
        let picture = proto::files::FileId::from_bytes([7; proto::files::FileId::LEN]);
        let server = terms::ServerUrl::parse("wss://chat.example.invalid/v1").unwrap();
        let row = platform::state::Row {
            server,
            name: None,
            colour: None,
            picture: Some(picture),
            identity: None,
            pin: None,
        };
        let opened: [screens::Opening; 1] = opened_at(vec![row]).try_into().unwrap();
        let [screens::Opening::Server(opened)] = opened else { panic!("one server, no account") };
        assert_eq!(opened.picture, Some(picture), "the cache can show it before a greeting");
    }
}
