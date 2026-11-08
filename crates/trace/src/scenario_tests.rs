//! Tests for scenario compilation (macros, encrypted bots).

use crate::scenario::{Action, Scenario, Step};

fn compile(encrypt: bool) -> Vec<Step> {
    let text = format!(
        "name = \"t\"\n[[bots]]\nname = \"a\"\nticket = \"x\"\nencrypt = {encrypt}\n\
         [[steps]]\nbot = \"a\"\nmacro = \"login\"\n"
    );
    toml::from_str::<Scenario>(&text)
        .unwrap()
        .compile()
        .unwrap()
}

fn sent(steps: &[Step]) -> Vec<u16> {
    steps
        .iter()
        .filter_map(|s| match s.action {
            Action::Send { header, .. } => Some(header),
            _ => None,
        })
        .collect()
}

#[test]
fn plain_login_has_no_generatekey() {
    // 206 INIT_CRYPTO, 813 UNIQUEID, 204 SSO: the macro the existing traces were recorded with.
    assert_eq!(sent(&compile(false)), [206, 813, 204]);
}

#[test]
fn encrypted_login_negotiates_the_key_and_waits_for_the_answer() {
    let steps = compile(true);
    assert_eq!(sent(&steps), [206, 2002, 813, 204]);
    let key = steps
        .iter()
        .find(|s| matches!(s.action, Action::Send { header: 2002, .. }))
        .unwrap();
    // SECRET_KEY: the bot must be switched to the cipher before the next step sends.
    assert_eq!(key.wait_for, [1]);
}

#[test]
fn scenarios_without_the_encrypt_option_still_load() {
    let text = "name = \"t\"\n[[bots]]\nname = \"a\"\nticket = \"x\"\n[[steps]]\nbot = \"a\"\nconnect = true\n";
    assert!(!toml::from_str::<Scenario>(text).unwrap().bots[0].encrypt);
}

#[test]
fn rcon_steps_compile_and_encode_like_java_reads_them() {
    let text = "name = \"t\"\n[[bots]]\nname = \"a\"\nticket = \"x\"\n[[steps]]\nbot = \"a\"\n\
                rcon = \"infobuspoll\"\nrcon_values = { pollId = \"1\" }\n";
    let steps = toml::from_str::<Scenario>(text).unwrap().compile().unwrap();
    let Action::Rcon { header, values } = &steps[0].action else {
        panic!("not an rcon step");
    };
    assert_eq!(header, "infobuspoll");
    // Length, then the header, one parameter, its key and value: each string length-prefixed.
    let frame = crate::runner::rcon_frame(header, values);
    let mut expected = 34i32.to_be_bytes().to_vec();
    for part in [
        &11i32.to_be_bytes()[..],
        b"infobuspoll",
        &1i32.to_be_bytes(),
    ] {
        expected.extend_from_slice(part);
    }
    for part in [
        &6i32.to_be_bytes()[..],
        b"pollId",
        &1i32.to_be_bytes(),
        b"1",
    ] {
        expected.extend_from_slice(part);
    }
    assert_eq!(frame, expected);
    let both = "name = \"t\"\n[[bots]]\nname = \"a\"\nticket = \"x\"\n[[steps]]\nbot = \"a\"\n\
                rcon = \"x\"\nsend = \"WALK\"\n";
    assert!(toml::from_str::<Scenario>(both).unwrap().compile().is_err());
}
