use vodozemac::megolm::GroupSession;

fn main() -> anyhow::Result<()> {
    let group_session = GroupSession::new();
    println!(
        "Group Session Pickle: {}",
        serde_json::to_string(&group_session.pickle())?
    );

    println!("Session Key: {}", group_session.session_key().to_base64());

    Ok(())
}
