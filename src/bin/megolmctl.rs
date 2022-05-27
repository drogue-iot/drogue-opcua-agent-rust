use olm_rs::PicklingMode;
use vodozemac::megolm::GroupSession;

const PICKLE_KEY: [u8; 32] = [0u8; 32];

fn main() -> anyhow::Result<()> {
    let group_session = GroupSession::new();
    println!(
        "Group Session Pickle: {}",
        serde_json::to_string(&group_session.pickle())?
    );

    println!(
        "Session Pickle: {}",
        group_session.pickle().encrypt(&PICKLE_KEY)
    );
    println!("Session Key: {}", group_session.session_key().to_base64());

    let group_session = olm_rs::outbound_group_session::OlmOutboundGroupSession::new();
    println!(
        "Session Pickle (plain): {}",
        group_session.pickle(PicklingMode::Unencrypted)
    );
    println!(
        "Session Pickle (enc): {}",
        group_session.pickle(PicklingMode::Encrypted {
            key: PICKLE_KEY.to_vec()
        })
    );
    println!("Session Key: {}", group_session.session_key());

    Ok(())
}
