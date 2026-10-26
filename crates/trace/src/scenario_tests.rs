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
