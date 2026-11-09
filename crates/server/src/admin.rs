//! The first administrator: a new hotel has nobody who can open the housekeeping, so at startup,
//! when no user has rank 8, the server creates one with a random password and prints it.

use std::path::Path;

use anyhow::Result;
use rusbbo_storage::{Store, User};

/// Housekeeping rank with every permission (Administrator).
const ADMIN_RANK: u8 = 8;
/// A look for the admin when the figure data cannot give a random one.
const FALLBACK_FIGURE: &str = "hd-180-1.ch-210-66.lg-270-82.sh-290-80.hr-100-7";
/// No look-alike characters (0/O, 1/l/I), so the password is easy to type from the console.
const ALPHABET: &[u8] = b"abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";

pub struct Created {
    pub username: String,
    pub password: String,
}

/// Creates `admin` (or `admin2`, … when taken) with rank 8 when no user has rank 8 yet.
pub fn ensure_admin(store: &Store, figuredata: &Path) -> Result<Option<Created>> {
    if store.users()?.iter().any(|u| u.rank >= ADMIN_RANK) {
        return Ok(None);
    }
    let mut username = "admin".to_string();
    let mut n = 1;
    while store.user_by_name(&username)?.is_some() {
        n += 1;
        username = format!("admin{n}");
    }
    let password = random_password(14);
    let figure = Some(crate::web::figure::Figures::load(figuredata).random("M", false))
        .filter(|f| !f.is_empty())
        .unwrap_or_else(|| FALLBACK_FIGURE.to_string());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let mut w = store.write()?;
    let user = User {
        id: w.next_user_id()?,
        username: username.clone(),
        password: crate::game::password::hash(&password)?,
        figure,
        sex: "M".into(),
        rank: ADMIN_RANK,
        created_at: now,
        updated_at: now,
        last_online: now,
        ..User::table_defaults()
    };
    w.put_user(&user)?;
    w.commit()?;
    Ok(Some(Created { username, password }))
}

fn random_password(len: usize) -> String {
    use rand::RngExt;
    let mut rng = rand::rng();
    (0..len)
        .map(|_| ALPHABET[rng.random_range(0..ALPHABET.len())] as char)
        .collect()
}

/// The console notice with the new admin's credentials.
pub fn banner(created: &Created, site: &str) -> String {
    let line = "=".repeat(72);
    format!(
        "\n{line}\n  No administrator yet, so one was created:\n\n    \
         username  {}\n    password  {}\n\n  \
         Log in on the website or the launcher. The housekeeping is at\n  \
         {site}/allseeingeye/hk; change the password in the website's settings.\n{line}\n",
        created.username, created.password
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_one_admin_once() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::create(&dir.path().join("t.redb")).unwrap();
        let mut taken = User::table_defaults();
        taken.id = 1;
        taken.username = "admin".into();
        store.put_user(&taken).unwrap();

        let created = ensure_admin(&store, Path::new("missing.xml"))
            .unwrap()
            .unwrap();
        assert_eq!(created.username, "admin2");
        assert_eq!(created.password.len(), 14);
        let user = store.user_by_name("admin2").unwrap().unwrap();
        assert_eq!(user.rank, 8);
        assert_eq!(user.figure, FALLBACK_FIGURE);
        assert!(crate::game::password::matches(
            &user.password,
            &created.password
        ));

        assert!(
            ensure_admin(&store, Path::new("missing.xml"))
                .unwrap()
                .is_none()
        );
    }
}
