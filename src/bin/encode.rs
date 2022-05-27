use olm_rs::PicklingMode;
use vodozemac::megolm::{
    GroupSession, GroupSessionPickle, InboundGroupSession, MegolmMessage, SessionKey,
};

const PICKLE_KEY: [u8; 32] = [0u8; 32];

fn main() -> anyhow::Result<()> {
    let pickle = r#"
{"ratchet":{"inner":[86,42,206,31,157,234,222,82,177,215,95,19,185,127,101,127,41,195,70,2,51,50,195,29,31,30,252,119,72,112,121,231,245,214,230,151,139,80,186,203,178,212,141,8,235,45,103,25,23,105,73,158,114,31,47,65,227,110,116,6,71,202,8,92,181,84,222,81,183,186,244,182,66,105,100,11,238,36,137,215,45,57,97,255,159,31,243,152,250,58,98,200,223,217,120,214,147,34,65,252,181,129,0,180,16,124,101,191,80,6,227,56,194,242,216,35,46,105,1,244,152,40,191,19,153,189,57,165],"counter":0},"signing_key":{"Normal":[20,143,242,90,32,50,204,9,163,76,15,172,205,233,134,175,111,30,251,0,205,115,201,132,251,101,26,122,155,254,229,135]}}
"#;
    let pickle = "8ajgfJVsSqCTdH1s8Z0FxOe8MQtSpDrtoJNCfyq3T1+hE/VU7bYLcgaUCw2zgASKbgofNGNFaa3BntynzJ1A4p623pLwredFMcJyb/UM1+IlPYsgT4Y38qaaRThZn7Pi10LF1Rgo4c2izHJGEy2oXz3c6i+SU4+/ykH5UF3YAUh99AgbbkXH+SD+uUGm2fUkXBtp9/BozkzXmCf3ktKQfK6ficp9rOd0varhzxTXzt0kwlHbPoFLDwH5Uo4mSv+rClxA1x/g+1xjMGihETZzEMNlGnN+fEqpWr9vJtEfZBniNkEGt4GiDbTnCfCPjndi8QJFOGwXTek";

    //let mut session: GroupSession = serde_json::from_str::<GroupSessionPickle>(&pickle)?.into();
    let mut session: GroupSession = GroupSessionPickle::from_encrypted(pickle, &PICKLE_KEY)?.into();
    //let mut session: GroupSession = InboundGroupSession::from_libolm_pickle()

    let payload = "foo";

    let msg = session.encrypt(payload);
    println!("Message (vodozemac): {}", msg.to_base64());

    //let pickle = base64::encode(pickle);
    let session = olm_rs::outbound_group_session::OlmOutboundGroupSession::unpickle(
        pickle.into(),
        PicklingMode::Encrypted {
            key: PICKLE_KEY.to_vec(),
        },
    )?;

    println!("Encrypting");
    let msg = session.encrypt(payload);
    println!("Message (libolm): {}", msg);

    Ok(())
}
