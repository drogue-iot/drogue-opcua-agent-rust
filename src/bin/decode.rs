use vodozemac::megolm::{
    GroupSession, GroupSessionPickle, InboundGroupSession, MegolmMessage, SessionKey,
};

fn main() -> anyhow::Result<()> {
    let key = "AgAAAABm8+ihlLuDnio1ijC8neRHBmXGdoAeNqfOJxc+MqPCC1irnCK+YayEJe6zvgu7Z1L8l9nV0sUzaUsBAD70ZXn21kmrGfSs+RJFy6gLla1rCfzx3VCwjfMsNW8quYgocg7hiN79Vt/f23tNO3FXMy0ljq8K4jub1ZCtnUl50WHT35IWBOp1NOG+Us7EPoq7+ISdFNjpXKRd6Ip4EGm24u0z8R1iYhkU485fpGYj9AT54bzmq3I8ehKDNqYaRq+FAXT2vy0OwfXDeQt2OWdsN1HPWfwIdCspvHuT3Nx1M0IDDw";
    let msg = "Awg1EpAGTo0xWHxn8OF9crKuapZecy9LawE7iV949kWnhVTizr9PJdl8KVAvQV0j1IdnYIuduAldptjaeCip+Yh1FoXntH5H5BLhgb0J/3ejTPCwXwkJJBNBsixTIA3oDGvZsHoJmYw39OgsLQJPnNEkIzyHXRGyx+OdybWzusyBD0jIDFSwMlnj/bWGuVdrbZWGkDPDlEGpWg5px5deb4tVNtt0ydYhCGpliOcguvh/k00EVqvFtNbxRxvt9h/PWs5zYwi0Eyi0GapKhY+GL7kxqEPJEhM/KFK1M/UZ+4MOzodluJzbSYopzzHEKgIwGV+fE8Jkbs76oMjg/WVv5OZKnLAoBVGxfV/tzwB4SgRm+miBvN+6ieAr4RhJPIwA6hhEPwB+zRZhdlP0k+9B8iSAmqNPtzu1Yut4jHs13yws06Vf0bdGxSjHFFZRAYBnYet25h3h1yeJ3NJBqsTBpue6jG8CH05dK24Zwi/SDmfBXo7ihnyxSMQNnTwvcPB2AntBjZXhfmM4HVQG8a0HmfFopRP2IZTmaOGWSP7kWi+7FG40klhY8lHQccIBOASP/TuiPaGwncXekNp6wRbr2SZZWTmjQ4g5nHQOmraEdFwAYJe8LAeYgSiKp5fALlKMtXTh139AdiapVmpq42WMAHEjgGstUAMtlhN3id6C2q7E+FqLLzlHFU8RgynJioOUTWfsk3hdWYGnQOWXENV5PaBMDQ5T7ssY6IQXQSXR5G378VF1DuvB8lM4jbNLlsn0mAz7LFpVJcjJnTjEKHKWCEh0+ps9s1UE1+GLZQIIk7nf6bmskuCBa9umq9pe9GN9NOkoUcouLAC43cgX+wIHdiw9VWQRQFw+xmSyjL94qKTSvNNPq4/XUgIfHMi9zj11ArRHPw9BokWwQIZFD+6sdQvJdwYQvp4F3giLky/mjbCBQYEM/gMM8pMQWRSke9QbHVXDXmR9rWO29pWwT05VdZfgYQSRjug/D3a/+ViDUpQPASqXZp9ItZEJkbsyFaCRpSynV64gVfR64SGmleVPYq5DKNHzYklfpFaza66u/pbxN+uQI7DQDdikdO1h8Q3xNUUMcZrlyIAcZPnnMuSLOxit2sngNEYobctdaoLmiJGVL6TAl1WdwVmFr4FCDQ==";
    let msg = "AwgDEpAG6jJvf52I2W9gR3+6ODwQqNzjZp7Z+fq+eyfnVQ9oPVZg/P/AuX10Zx38QRv6ezQ3DflO95Fv7uB6LM0cNwYbepZHPE4df/uGSHyYkpWXcoaqsGOSzI2H0XV4tCN7AYKCxnGbuoOJjzW9n9Dsnp/pcaRFAASwFoQsLMH91ZVAzWA6QqtWyAYC9slA26Mt2kWkxLWQ4jYdw9ZwV56FwWfuL9FYe+IYizInoUNpRIT6o1dDpEmutCudWgv8DxK2RVNtdF3q4znguqrwU6DbYBPVJBRZXaJM7e1MbFbkev0SKIYarIGvCAmYGFCpLRZvoUsvAMrGVK4Nnj8SJjZh0nMW5zVVnYnIyzCkV6XFxzqzyAx6Iih5eDUOYenEJzkCBr+6EPu12kmmbHFcYTcvgb+6vaMVL3Wbl60ucJ6mZeHidJn/qsTV60VN6eteteaaGxSzn4rOAgkDMs9ARjBpt/sepEvbhSiCA2CdiFLAeA7D9f/QcOn7RS6A8s0hyzyOaDVjWvDrl5L9bfvDzmzy8Qg84hJmJyiimZO3eTcwqwQbV98fhe/Nx3yYLfRUz4hKNZybtrM+dFeN4/aPKDg/mm1PKGp9qVLod9aPwZndmhLtGd3Yx26+axFbu9iFzTQfjukqgLJGavYF8OMF3NLIcld75buHC07PyxOWnks9kglSKh7totsSp0PZzCUgkis7mwLgcXV/NSpV+QthJ7ImGLSV6TXsNccpRLuUi45RCuq+zBak4rhEDhaLijSI/jWJaZbWjt6rKcbWD1Q+Z8g9psSxQyoFP6jdU5MA+6uA5Dj4hacvZSM8uWKoeuo/S5xlmq1EmENpvntJ5871KRF6F+GCBIyM4bT4XsBiZHUrdgiy1iMASmvdq3Z9xU8h+ZKGqVIjVdX2f2gYblWB5H0mG+8PPYGX0siGs9c+86jnQe8BOgxr1EA8Yy5tYDPot89uJY/CdK6quUbtue+cZu0klW6mRCh6anbOApffc7OcombJ7qCVtaKBW+vhBeh2/CI4uF+l7XG0aM2gKte6tedpcuLBDEIIhY1LWj4iQEYVrF6sRjWnTaRCAK88fKjpcQuWX4h2dGDWviMbiIb2pPw1Zr34hO2IPc8fTBBxw647WVgcYyi0gMl7W2EFCA==";
    //let msg = "AwgAEhDxIY+zmo+UOCTNHuGyHU/VzrTB3ToaLWlIXdLjbLEL9ltRydVJe0z/c+CCZYyOffZbY7MPHUIAOqNvVYvwX+8PWc75QWntv1MSuncic30dPXg+p9jgSvEJ";

    let mut session = InboundGroupSession::new(&SessionKey::from_base64(key)?);
    //println!("Session (vodozemac): {session:?}");
    let payload = session.decrypt(&MegolmMessage::from_base64(msg)?);
    println!("Decoded (vodozemac): {payload:?}");

    let session = olm_rs::inbound_group_session::OlmInboundGroupSession::new(key)?;
    //println!("Session (libolm): {session:?}");
    let payload = session.decrypt(msg.into());
    println!("Decoded (libolm): {payload:?}");

    Ok(())
}
