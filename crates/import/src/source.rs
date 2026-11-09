//! Where the Havana database comes from: a MariaDB/MySQL server ([`MySql`]), or a `.sql` dump
//! file such as Havana's own `havana.sql` ([`Dump`]), so no database server is needed at all.
//!
//! The importer asks both with the same MariaDB queries. A dump is loaded into an in-memory SQLite
//! database (its `CREATE TABLE`s and `INSERT`s parsed here), and the few MariaDB functions the
//! queries use (`UNIX_TIMESTAMP`, `TIMESTAMPDIFF`, `IF`) are provided on top. Rows come back the
//! way MariaDB's text protocol sends them (every value as text), so the importer's conversions are
//! the same for both.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use mysql_common::constants::ColumnType;
use mysql_common::packets::Column;
use mysql_common::row::Row;
use mysql_common::row::convert::FromRow;
use mysql_common::value::Value;

/// A Havana database to read.
pub trait Source {
    /// The rows of a (MariaDB) `SELECT`.
    fn rows(&mut self, sql: &str) -> Result<Vec<Row>>;
    /// The table's `AUTO_INCREMENT` counter: the id its next `INSERT` gets (`None` without one).
    fn auto_increment(&mut self, table: &str) -> Result<Option<u64>>;
    fn has_table(&mut self, table: &str) -> Result<bool>;
}

/// The `mysql` crate's query helpers, for any [`Source`].
pub trait SourceExt: Source {
    fn query<T: FromRow>(&mut self, sql: impl AsRef<str>) -> Result<Vec<T>> {
        let sql = sql.as_ref();
        self.rows(sql)?
            .into_iter()
            .map(|row| {
                mysql_common::row::convert::from_row_opt(row)
                    .map_err(|e| anyhow::anyhow!("{sql}: {e}"))
            })
            .collect()
    }

    fn query_first<T: FromRow>(&mut self, sql: impl AsRef<str>) -> Result<Option<T>> {
        Ok(self.query(sql)?.into_iter().next())
    }
}

impl<S: Source + ?Sized> SourceExt for S {}

// ---- MariaDB / MySQL ---------------------------------------------------------------------------

#[cfg(feature = "mysql")]
pub struct MySql(pub mysql::PooledConn);

#[cfg(feature = "mysql")]
impl MySql {
    pub fn connect(url: &str) -> Result<Self> {
        let pool = mysql::Pool::new(mysql::Opts::from_url(url).context("parsing the MySQL URL")?)
            .context("connecting to MariaDB")?;
        Ok(MySql(pool.get_conn()?))
    }
}

#[cfg(feature = "mysql")]
impl Source for MySql {
    fn rows(&mut self, sql: &str) -> Result<Vec<Row>> {
        use mysql::prelude::Queryable;
        Ok(self.0.query(sql)?)
    }

    fn auto_increment(&mut self, table: &str) -> Result<Option<u64>> {
        use mysql::prelude::Queryable;
        let next: Option<Option<u64>> = self.0.exec_first(
            "SELECT AUTO_INCREMENT FROM information_schema.TABLES \
             WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ?",
            (table,),
        )?;
        Ok(next.flatten())
    }

    fn has_table(&mut self, table: &str) -> Result<bool> {
        use mysql::prelude::Queryable;
        let found: Option<String> = self.0.exec_first(
            "SELECT TABLE_NAME FROM information_schema.TABLES \
             WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ?",
            (table,),
        )?;
        Ok(found.is_some())
    }
}

// ---- SQL dump ----------------------------------------------------------------------------------

struct ColumnDef {
    name: String,
    integer: bool,
    timestamp: bool,
    auto: bool,
    /// The value an `INSERT` that leaves the column out gets.
    default: Lit,
}

/// A column definition (`` `name` int(11) NOT NULL DEFAULT 0 ``); `None` for anything else in
/// a `CREATE TABLE` (keys, constraints).
fn parse_column(def: &str) -> Result<Option<ColumnDef>> {
    let Some(quote) = def.chars().next().filter(|c| *c == '`' || *c == '"') else {
        return Ok(None);
    };
    let end = def[1..].find(quote).context("bad column name")? + 1;
    let name = def[1..end].to_string();
    let rest = def[end + 1..].trim();
    // Keywords are looked for outside quoted text (an enum's values, a COMMENT).
    let upper = String::from_utf8(mask_quoted(rest))
        .unwrap_or_default()
        .to_ascii_uppercase();
    let kind = upper.split(['(', ' ']).next().unwrap_or("").to_string();
    let integer = matches!(
        kind.as_str(),
        "INT"
            | "TINYINT"
            | "SMALLINT"
            | "MEDIUMINT"
            | "BIGINT"
            | "INTEGER"
            | "BIT"
            | "BOOL"
            | "BOOLEAN"
            | "YEAR"
    );
    let not_null = upper.contains("NOT NULL");
    let default = match upper.find(" DEFAULT ") {
        Some(at) => match parse_literal(rest[at + " DEFAULT ".len()..].trim_start()) {
            Some((lit, _)) => lit,
            // current_timestamp(), now()...
            None => Lit::Text(now_local()),
        },
        None if !not_null => Lit::Null,
        None if integer => Lit::Int(0),
        None if matches!(kind.as_str(), "DATETIME" | "TIMESTAMP") => {
            Lit::Text("0000-00-00 00:00:00".into())
        }
        None if kind == "DATE" => Lit::Text("0000-00-00".into()),
        None => Lit::Text(String::new()),
    };
    Ok(Some(ColumnDef {
        name,
        integer,
        timestamp: kind == "TIMESTAMP",
        auto: upper.contains("AUTO_INCREMENT"),
        default,
    }))
}

/// `text`'s bytes with everything inside quotes replaced by `x` (same offsets).
fn mask_quoted(text: &str) -> Vec<u8> {
    let mut out = text.as_bytes().to_vec();
    let mut quote: Option<u8> = None;
    let mut escaped = false;
    for b in out.iter_mut() {
        match quote {
            Some(q) => {
                let c = *b;
                *b = b'x';
                if escaped {
                    escaped = false;
                } else if c == b'\\' {
                    escaped = true;
                } else if c == q {
                    quote = None;
                    *b = c;
                }
            }
            None if *b == b'\'' || *b == b'"' => quote = Some(*b),
            None => {}
        }
    }
    out
}

/// A statement with its MySQL string literals rewritten for SQLite (`'it\'s'` → `'it''s'`,
/// `"text"` → `'text'`).
fn sqlite_strings(statement: &str) -> String {
    let mut out = String::with_capacity(statement.len());
    let mut i = 0;
    while i < statement.len() {
        let rest = &statement[i..];
        let c = rest.chars().next().unwrap_or(' ');
        if c == '\'' || c == '"' {
            if let Some((text, used)) = parse_string(rest) {
                out.push('\'');
                out.push_str(&text.replace('\'', "''"));
                out.push('\'');
                i += used;
                continue;
            }
        }
        out.push(c);
        i += c.len_utf8();
    }
    out
}

fn sqlite_value(lit: &Lit) -> rusqlite::types::Value {
    match lit {
        Lit::Null => rusqlite::types::Value::Null,
        Lit::Int(i) => rusqlite::types::Value::Integer(*i),
        Lit::Text(t) => rusqlite::types::Value::Text(t.clone()),
        Lit::Blob(b) => rusqlite::types::Value::Blob(b.clone()),
    }
}

struct Table {
    columns: Vec<ColumnDef>,
    auto_column: Option<usize>,
    /// The `AUTO_INCREMENT=` table option.
    auto_increment: Option<u64>,
}

/// A `.sql` dump (mysqldump, HeidiSQL, phpMyAdmin...) loaded into memory.
pub struct Dump {
    db: rusqlite::Connection,
    tables: HashMap<String, Table>,
}

#[derive(Clone, Debug, PartialEq)]
enum Lit {
    Null,
    Int(i64),
    /// Numbers that are not integers keep their text ("1.50"), as MariaDB sends them.
    Text(String),
    Blob(Vec<u8>),
}

impl Dump {
    pub fn load(sql: &str) -> Result<Self> {
        let db = rusqlite::Connection::open_in_memory()?;
        register_functions(&db)?;
        let mut dump = Dump {
            db,
            tables: HashMap::new(),
        };
        dump.apply(sql)?;
        Ok(dump)
    }

    /// Runs more SQL on the loaded dump: a migration (`CREATE TABLE`, `INSERT`, `UPDATE`,
    /// `DELETE`, `DROP TABLE`, `ALTER TABLE ... ADD COLUMN`; other statements are ignored).
    pub fn apply(&mut self, sql: &str) -> Result<()> {
        // HeidiSQL and mysqldump write TIMESTAMP columns in UTC (`SET TIME_ZONE='+00:00'`);
        // MariaDB would show them in its own time zone, so they are turned into local time.
        let utc = sql.contains("TIME_ZONE='+00:00'");
        self.db.execute_batch("BEGIN")?;
        for statement in statements(sql) {
            let head: String = statement
                .split_whitespace()
                .take(3)
                .collect::<Vec<_>>()
                .join(" ")
                .to_ascii_uppercase();
            let result = if head.starts_with("CREATE TABLE") {
                self.create_table(&statement)
            } else if head.starts_with("INSERT") || head.starts_with("REPLACE") {
                self.insert(&statement, utc)
            } else if head.starts_with("DROP TABLE") {
                table_name_after(&statement, "TABLE").and_then(|name| {
                    self.db
                        .execute_batch(&format!("DROP TABLE IF EXISTS \"{name}\""))?;
                    self.tables.remove(&name);
                    Ok(())
                })
            } else if head.starts_with("ALTER TABLE") {
                self.alter_table(&statement)
            } else if head.starts_with("UPDATE") || head.starts_with("DELETE") {
                self.db
                    .execute_batch(&sqlite_strings(&statement))
                    .map_err(Into::into)
            } else {
                Ok(())
            };
            result.with_context(|| first_line(&statement).to_string())?;
        }
        self.db.execute_batch("COMMIT")?;
        Ok(())
    }

    pub fn table_count(&self) -> usize {
        self.tables.len()
    }

    /// `ALTER TABLE t ADD [COLUMN] [IF NOT EXISTS] c type ...`; indexes and type changes do not
    /// matter here.
    fn alter_table(&mut self, statement: &str) -> Result<()> {
        let name = table_name_after(statement, "TABLE")?;
        let quoted = statement.find(&name).context("table name")? + name.len();
        let clauses = statement[quoted..].trim_start_matches(['`', '"']);
        for clause in split_top_level(clauses, ',') {
            let clause = clause.trim();
            let upper = clause.to_ascii_uppercase();
            let Some(mut def) = upper.strip_prefix("ADD").map(|_| clause[3..].trim_start()) else {
                continue;
            };
            if def.to_ascii_uppercase().starts_with("COLUMN") {
                def = def[6..].trim_start();
            }
            if def.to_ascii_uppercase().starts_with("IF NOT EXISTS") {
                def = def[13..].trim_start();
            }
            let Some(column) = parse_column(def)? else {
                continue; // ADD INDEX, ADD KEY, ADD CONSTRAINT...
            };
            let table = self
                .tables
                .get_mut(&name)
                .with_context(|| format!("ALTER of unknown table {name}"))?;
            if table.columns.iter().any(|c| c.name == column.name) {
                continue;
            }
            self.db.execute_batch(&format!(
                "ALTER TABLE \"{name}\" ADD COLUMN \"{}\"{}",
                column.name,
                if column.integer { " INTEGER" } else { "" }
            ))?;
            self.db.execute(
                &format!("UPDATE \"{name}\" SET \"{}\" = ?", column.name),
                [sqlite_value(&column.default)],
            )?;
            table.columns.push(column);
        }
        Ok(())
    }

    fn create_table(&mut self, statement: &str) -> Result<()> {
        let name = table_name_after(statement, "TABLE")?;
        if self.tables.contains_key(&name)
            && statement
                .to_ascii_uppercase()
                .split_whitespace()
                .take(5)
                .collect::<Vec<_>>()
                .join(" ")
                .contains("IF NOT EXISTS")
        {
            return Ok(());
        }
        let open = statement.find('(').context("no column list")?;
        let close = statement.rfind(')').context("no column list")?;
        let options = &statement[close + 1..];
        let mut table = Table {
            columns: Vec::new(),
            auto_column: None,
            auto_increment: options
                .to_ascii_uppercase()
                .split("AUTO_INCREMENT=")
                .nth(1)
                .and_then(|rest| {
                    rest.chars()
                        .take_while(char::is_ascii_digit)
                        .collect::<String>()
                        .parse()
                        .ok()
                }),
        };
        for def in split_top_level(&statement[open + 1..close], ',') {
            let Some(column) = parse_column(def.trim())? else {
                continue; // PRIMARY KEY, KEY, UNIQUE, CONSTRAINT, INDEX...
            };
            if column.auto {
                table.auto_column = Some(table.columns.len());
            }
            table.columns.push(column);
        }
        let columns: Vec<String> = table
            .columns
            .iter()
            .map(|c| format!("\"{}\"{}", c.name, if c.integer { " INTEGER" } else { "" }))
            .collect();
        self.db
            .execute_batch(&format!("DROP TABLE IF EXISTS \"{name}\""))?;
        self.db
            .execute_batch(&format!("CREATE TABLE \"{name}\" ({})", columns.join(", ")))?;
        self.tables.insert(name, table);
        Ok(())
    }

    fn insert(&mut self, statement: &str, utc: bool) -> Result<()> {
        let name = table_name_after(statement, "INTO")?;
        let table = self
            .tables
            .get_mut(&name)
            .with_context(|| format!("INSERT into unknown table {name}"))?;
        let after_name = &statement[statement.find(&name).context("table name")? + name.len()..];
        let after_name = after_name.trim_start_matches(['`', '"']).trim_start();
        let values_at = after_name
            .to_ascii_uppercase()
            .find("VALUES")
            .context("no VALUES")?;
        // The column list, or every column in order.
        let listed: Vec<usize> = if after_name.starts_with('(') {
            let list = &after_name[1..after_name.find(')').context("bad column list")?];
            list.split(',')
                .map(|c| {
                    let c = c.trim().trim_matches(['`', '"']);
                    table
                        .columns
                        .iter()
                        .position(|d| d.name == c)
                        .with_context(|| format!("unknown column {c}"))
                })
                .collect::<Result<_>>()?
        } else {
            (0..table.columns.len()).collect()
        };
        let placeholders = vec!["?"; table.columns.len()].join(", ");
        let sql = format!("INSERT INTO \"{name}\" VALUES ({placeholders})");
        let mut stmt = self.db.prepare_cached(&sql)?;
        let mut rest = after_name[values_at + "VALUES".len()..].trim_start();
        while let Some(tuple) = rest.strip_prefix('(') {
            let mut row: Vec<Lit> = table.columns.iter().map(|c| c.default.clone()).collect();
            let mut given = vec![false; row.len()];
            let mut cursor = tuple;
            for (n, &column) in listed.iter().enumerate() {
                cursor = cursor.trim_start();
                if n > 0 {
                    cursor = cursor.strip_prefix(',').context("expected ,")?.trim_start();
                }
                let (lit, used) = parse_literal(cursor).with_context(|| {
                    format!("bad value near {:?}", &cursor[..cursor.len().min(40)])
                })?;
                row[column] = lit;
                given[column] = true;
                cursor = &cursor[used..];
            }
            rest = cursor
                .trim_start()
                .strip_prefix(')')
                .context("expected )")?
                .trim_start();
            rest = rest.strip_prefix(',').unwrap_or(rest).trim_start();

            if let Some(auto) = table.auto_column {
                let id = match &row[auto] {
                    Lit::Int(id) if given[auto] && *id != 0 => *id as u64,
                    _ => {
                        let next = table.auto_increment.unwrap_or(1);
                        row[auto] = Lit::Int(next as i64);
                        next
                    }
                };
                table.auto_increment = Some(table.auto_increment.unwrap_or(1).max(id + 1));
            }
            if utc {
                for (value, column) in row.iter_mut().zip(&table.columns) {
                    if let (true, Lit::Text(text)) = (column.timestamp, &*value) {
                        *value = Lit::Text(utc_to_local(text));
                    }
                }
            }
            stmt.execute(rusqlite::params_from_iter(row.iter().map(sqlite_value)))?;
        }
        Ok(())
    }
}

impl Source for Dump {
    fn rows(&mut self, sql: &str) -> Result<Vec<Row>> {
        let translated = translate(sql);
        let mut stmt = self
            .db
            .prepare(&translated)
            .with_context(|| format!("in the dump: {sql}"))?;
        let columns: Arc<[Column]> = stmt
            .column_names()
            .iter()
            .map(|name| Column::new(ColumnType::MYSQL_TYPE_VAR_STRING).with_name(name.as_bytes()))
            .collect();
        let count = columns.len();
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let mut values = Vec::with_capacity(count);
            for i in 0..count {
                use rusqlite::types::ValueRef;
                values.push(match row.get_ref(i)? {
                    ValueRef::Null => Value::NULL,
                    ValueRef::Integer(n) => Value::Bytes(n.to_string().into_bytes()),
                    ValueRef::Real(f) => Value::Bytes(f.to_string().into_bytes()),
                    ValueRef::Text(t) | ValueRef::Blob(t) => Value::Bytes(t.to_vec()),
                });
            }
            out.push(mysql_common::row::new_row(values, columns.clone()));
        }
        Ok(out)
    }

    fn auto_increment(&mut self, table: &str) -> Result<Option<u64>> {
        let Some(t) = self.tables.get(table) else {
            return Ok(None);
        };
        Ok(t.auto_column.map(|_| t.auto_increment.unwrap_or(1)))
    }

    fn has_table(&mut self, table: &str) -> Result<bool> {
        Ok(self.tables.contains_key(table))
    }
}

/// The MariaDB in the importer's queries, as SQLite: `IF(` is `iif(`, and `TIMESTAMPDIFF`'s unit
/// becomes a string.
fn translate(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let bytes = sql.as_bytes();
    let mut i = 0;
    while i < sql.len() {
        let word_start = i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
        if word_start && sql[i..].len() >= 3 && sql[i..i + 3].eq_ignore_ascii_case("IF(") {
            out.push_str("iif(");
            i += 3;
        } else if word_start
            && sql[i..]
                .to_ascii_uppercase()
                .starts_with("TIMESTAMPDIFF(SECOND,")
        {
            out.push_str("TIMESTAMPDIFF('SECOND',");
            i += "TIMESTAMPDIFF(SECOND,".len();
        } else {
            let c = sql[i..].chars().next().unwrap_or(' ');
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

fn register_functions(db: &rusqlite::Connection) -> Result<()> {
    use rusqlite::functions::FunctionFlags;
    let flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;
    db.create_scalar_function("UNIX_TIMESTAMP", 1, flags, |ctx| {
        let value: Option<String> = ctx.get(0).ok();
        Ok(value.and_then(|v| unix_timestamp(&v)))
    })?;
    db.create_scalar_function("TIMESTAMPDIFF", 3, flags, |ctx| {
        let unit: String = ctx.get(0)?;
        let (Ok(a), Ok(b)) = (ctx.get::<String>(1), ctx.get::<String>(2)) else {
            return Ok(None);
        };
        if !unit.eq_ignore_ascii_case("SECOND") {
            return Err(rusqlite::Error::UserFunctionError(
                format!("TIMESTAMPDIFF({unit}) is not supported").into(),
            ));
        }
        Ok(match (naive(&a), naive(&b)) {
            (Some(a), Some(b)) => Some((b - a).num_seconds()),
            _ => None,
        })
    })?;
    Ok(())
}

fn naive(text: &str) -> Option<chrono::NaiveDateTime> {
    let text = text.split('.').next()?.trim();
    chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S")
        .ok()
        .or_else(|| {
            chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d")
                .ok()
                .and_then(|d| d.and_hms_opt(0, 0, 0))
        })
}

/// MariaDB's `UNIX_TIMESTAMP(datetime)` in the local time zone: 0 for a zero date, NULL for
/// text that is not a date.
fn unix_timestamp(text: &str) -> Option<i64> {
    use chrono::TimeZone;
    if text.starts_with("0000-00-00") {
        return Some(0);
    }
    let wall = naive(text)?;
    let seconds = chrono::Local
        .from_local_datetime(&wall)
        .earliest()
        .map(|t| t.timestamp())?;
    Some(seconds.max(0))
}

fn utc_to_local(text: &str) -> String {
    match naive(text) {
        Some(wall) if !text.starts_with("0000") => wall
            .and_utc()
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
        _ => text.to_string(),
    }
}

fn now_local() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn first_line(statement: &str) -> &str {
    let line = statement.lines().next().unwrap_or("");
    &line[..line.len().min(100)]
}

/// The (back)quoted or bare name after `keyword` (`CREATE TABLE IF NOT EXISTS \`x\``,
/// `INSERT INTO \`x\``).
fn table_name_after(statement: &str, keyword: &str) -> Result<String> {
    let upper = statement.to_ascii_uppercase();
    let at = upper
        .find(&format!("{keyword} "))
        .with_context(|| format!("no {keyword}"))?
        + keyword.len();
    let mut rest = statement[at..].trim_start();
    for skip in ["IF NOT EXISTS", "IF EXISTS"] {
        if rest.to_ascii_uppercase().starts_with(skip) {
            rest = rest[skip.len()..].trim_start();
        }
    }
    let name: String = if let Some(quoted) = rest.strip_prefix(['`', '"']) {
        quoted
            .chars()
            .take_while(|c| *c != '`' && *c != '"')
            .collect()
    } else {
        rest.chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
            .collect()
    };
    // `db`.`table`
    let name = name.rsplit('.').next().unwrap_or(&name).to_string();
    if name.is_empty() {
        bail!("no table name");
    }
    Ok(name)
}

/// One SQL literal at the start of `text`, and how many bytes it took.
fn parse_literal(text: &str) -> Option<(Lit, usize)> {
    let bytes = text.as_bytes();
    let first = *bytes.first()?;
    if first == b'\'' || first == b'"' {
        let (s, used) = parse_string(text)?;
        return Some((Lit::Text(s), used));
    }
    if bytes.len() >= 4 && bytes[..4].eq_ignore_ascii_case(b"NULL") {
        return Some((Lit::Null, 4));
    }
    if text.starts_with("_binary") {
        let skip = "_binary".len();
        let trimmed = text[skip..].trim_start();
        let (lit, used) = parse_literal(trimmed)?;
        let lit = match lit {
            Lit::Text(s) => Lit::Blob(s.into_bytes()),
            other => other,
        };
        return Some((lit, text.len() - trimmed.len() + used));
    }
    if text.len() > 2 && (text.starts_with("0x") || text.starts_with("0X")) {
        let hex: String = text[2..]
            .chars()
            .take_while(char::is_ascii_hexdigit)
            .collect();
        let blob = (0..hex.len() / 2)
            .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok())
            .collect::<Option<Vec<u8>>>()?;
        return Some((Lit::Blob(blob), 2 + hex.len()));
    }
    if (first == b'b' || first == b'B') && bytes.get(1) == Some(&b'\'') {
        let end = text[2..].find('\'')? + 2;
        let value = i64::from_str_radix(&text[2..end], 2).ok()?;
        return Some((Lit::Int(value), end + 1));
    }
    let len = text
        .char_indices()
        .take_while(|(i, c)| {
            c.is_ascii_digit()
                || *c == '.'
                || ((*c == '-' || *c == '+') && *i == 0)
                || *c == 'e'
                || *c == 'E'
        })
        .count();
    if len == 0 {
        return None;
    }
    let number = &text[..len];
    Some((
        number
            .parse::<i64>()
            .map_or_else(|_| Lit::Text(number.to_string()), Lit::Int),
        len,
    ))
}

/// A quoted string with MySQL's backslash escapes and doubled quotes.
fn parse_string(text: &str) -> Option<(String, usize)> {
    let quote = text.chars().next()?;
    let mut out = String::new();
    let mut chars = text.char_indices().skip(1).peekable();
    while let Some((i, c)) = chars.next() {
        if c == '\\' {
            let (_, e) = chars.next()?;
            if matches!(e, '%' | '_') {
                out.push('\\'); // \% and \_ keep their backslash (LIKE escapes)
            }
            out.push(match e {
                '0' => '\0',
                'b' => '\u{8}',
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                'Z' => '\u{1a}',
                other => other,
            });
        } else if c == quote {
            if chars.peek().map(|(_, n)| *n) == Some(quote) {
                chars.next();
                out.push(quote);
            } else {
                return Some((out, i + c.len_utf8()));
            }
        } else {
            out.push(c);
        }
    }
    None
}

/// Splits on `sep` outside quotes and parentheses.
fn split_top_level(text: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' | '`' => quote = Some(c),
            '(' => depth += 1,
            ')' => depth -= 1,
            c if c == sep && depth == 0 => {
                parts.push(&text[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}

/// The dump's statements, without comments (`--`, `#`, `/* */`, including `/*!...*/`).
fn statements(sql: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut chars = sql.chars().peekable();
    let mut quote: Option<char> = None;
    let mut line_start = true;
    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            current.push(c);
            if c == '\\' && q != '`' {
                if let Some(n) = chars.next() {
                    current.push(n);
                }
            } else if c == q {
                if chars.peek() == Some(&q) {
                    current.push(chars.next().unwrap_or(q));
                } else {
                    quote = None;
                }
            }
            continue;
        }
        match c {
            '\'' | '"' | '`' => {
                quote = Some(c);
                current.push(c);
            }
            '-' if chars.peek() == Some(&'-') => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        break;
                    }
                }
                current.push('\n');
            }
            '#' if line_start => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        break;
                    }
                }
                current.push('\n');
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = ' ';
                for n in chars.by_ref() {
                    if prev == '*' && n == '/' {
                        break;
                    }
                    prev = n;
                }
                current.push(' ');
            }
            ';' => {
                let s = current.trim();
                if !s.is_empty() {
                    out.push(s.to_string());
                }
                current.clear();
            }
            _ => current.push(c),
        }
        line_start = c == '\n';
    }
    let s = current.trim();
    if !s.is_empty() {
        out.push(s.to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SQL: &str = r#"
-- comment; with a semicolon
/*!40103 SET TIME_ZONE='+00:00' */;
CREATE TABLE IF NOT EXISTS `t` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `name` varchar(50) NOT NULL DEFAULT 'a,b',
  `kind` enum('x','y') DEFAULT NULL,
  `price` decimal(10,2) NOT NULL DEFAULT 0.00,
  `at` datetime NOT NULL DEFAULT current_timestamp(),
  `disabled` tinyint(1) NOT NULL DEFAULT 0,
  PRIMARY KEY (`id`),
  KEY `name` (`name`)
) ENGINE=InnoDB AUTO_INCREMENT=10 DEFAULT CHARSET=utf8mb4;
INSERT INTO `t` (`id`, `name`, `kind`, `price`, `at`, `disabled`) VALUES
	(1, 'it\'s; ok', 'x', 1.50, '2024-01-02 03:04:05', 0),
	(2, 'two ''quotes'' \\ \n', NULL, 2, '0000-00-00 00:00:00', '1');
INSERT INTO `t` (`name`) VALUES ('defaults');
CREATE TABLE `empty` (`id` int(11) NOT NULL AUTO_INCREMENT, PRIMARY KEY (`id`));
"#;

    #[test]
    fn loads_and_queries() {
        let mut dump = Dump::load(SQL).unwrap();
        let rows: Vec<(u32, String, Option<String>, String, bool)> = dump
            .query("SELECT id, name, kind, price, disabled FROM t ORDER BY id")
            .unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows[0],
            (1, "it's; ok".into(), Some("x".into()), "1.50".into(), false)
        );
        assert_eq!(rows[1].1, "two 'quotes' \\ \n");
        assert!(rows[1].4);
        assert_eq!(rows[2], (10, "defaults".into(), None, "0.00".into(), false));
        assert_eq!(dump.auto_increment("t").unwrap(), Some(11));
        assert_eq!(dump.auto_increment("empty").unwrap(), Some(1));
        assert!(dump.has_table("empty").unwrap() && !dump.has_table("nope").unwrap());

        let off: Vec<u32> = dump
            .query("SELECT id FROM t WHERE disabled = 0 ORDER BY id")
            .unwrap();
        assert_eq!(off, vec![1, 10]);
        let zero: Option<i64> = dump
            .query_first("SELECT UNIX_TIMESTAMP(at) FROM t WHERE id = 2")
            .unwrap();
        assert_eq!(zero, Some(0));
        let ts: Option<String> = dump
            .query_first(
                "SELECT IF(IFNULL(UNIX_TIMESTAMP(at), 0) = 0 AND at > '1970-01-02', \
                 TIMESTAMPDIFF(SECOND, '1970-01-01 00:00:00', at), UNIX_TIMESTAMP(at)) AS a FROM t WHERE id = 1",
            )
            .unwrap();
        let expected = unix_timestamp("2024-01-02 03:04:05").unwrap();
        assert_eq!(ts, Some(expected.to_string()));
        let row = &dump
            .rows("SELECT *, CAST(UNIX_TIMESTAMP(at) AS SIGNED) AS at_ts FROM t")
            .unwrap()[0];
        assert_eq!(row.get::<i64, _>("at_ts"), Some(expected));
    }

    #[test]
    fn migrations() {
        let mut dump = Dump::load(SQL).unwrap();
        dump.apply(
            "CREATE TABLE IF NOT EXISTS `t` (`other` int(11));\n\
             UPDATE `t` SET `name`='o\\'k' WHERE `id` IN (1, 2);\n\
             DELETE FROM `t` WHERE `id` = 10;\n\
             ALTER TABLE `t` ADD COLUMN IF NOT EXISTS `pick` tinyint(1) NOT NULL DEFAULT 1 COMMENT 'a DEFAULT ¡x',\n\
               ADD INDEX `idx` (`pick`);",
        )
        .unwrap();
        let rows: Vec<(u32, String, u8)> = dump
            .query("SELECT id, name, pick FROM t ORDER BY id")
            .unwrap();
        assert_eq!(rows, vec![(1, "o'k".into(), 1), (2, "o'k".into(), 1)]);
    }

    #[test]
    fn literals() {
        assert_eq!(
            parse_literal("0x4142,"),
            Some((Lit::Blob(b"AB".to_vec()), 6))
        );
        assert_eq!(parse_literal("b'101')"), Some((Lit::Int(5), 6)));
        assert_eq!(
            parse_literal("-3.5e2,"),
            Some((Lit::Text("-3.5e2".into()), 6))
        );
        assert_eq!(parse_literal("NULL)"), Some((Lit::Null, 4)));
    }
}
