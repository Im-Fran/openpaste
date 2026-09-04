use anyhow::Result;
use sqlx::{any::AnyPoolOptions, AnyPool, Row};

#[derive(Clone)]
pub struct Db {
    pool: AnyPool,
    postgres: bool,
}

#[derive(serde::Serialize, Clone)]
pub struct Paste {
    pub id: String,
    pub content: Option<String>,
    pub storage_key: Option<String>,
    pub filename: Option<String>,
    pub content_type: String,
    pub size: i64,
    pub created_at: i64,
}

impl Paste {
    pub fn is_binary(&self) -> bool {
        self.storage_key.is_some()
    }
}

// ponytail: sqlx's Any driver forwards SQL verbatim, so `?` breaks on postgres.
// Naive rewrite is fine because every query here is a literal we control (no `?` inside strings).
fn q(sql: &str, postgres: bool) -> String {
    if !postgres {
        return sql.to_string();
    }
    let mut n = 0;
    sql.chars()
        .map(|c| {
            if c == '?' {
                n += 1;
                format!("${n}")
            } else {
                c.to_string()
            }
        })
        .collect()
}

impl Db {
    pub async fn connect(url: &str) -> Result<Self> {
        sqlx::any::install_default_drivers();
        if let Some(path) = url.strip_prefix("sqlite://").map(|p| p.split('?').next().unwrap_or(p)) {
            if let Some(dir) = std::path::Path::new(path).parent() {
                tokio::fs::create_dir_all(dir).await.ok();
            }
        }
        let pool = AnyPoolOptions::new().max_connections(5).connect(url).await?;
        // ponytail: one idempotent statement beats a migration runner; swap in sqlx::migrate!
        // the day the schema needs versioned upgrades.
        for stmt in include_str!("../schema.sql").split(";").filter(|s| !s.trim().is_empty()) {
            sqlx::query(stmt).execute(&pool).await?;
        }
        Ok(Db { pool, postgres: url.starts_with("postgres") })
    }

    pub async fn insert(&self, p: &Paste) -> Result<()> {
        let sql = q(
            "INSERT INTO pastes (id, content, storage_key, filename, content_type, size, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            self.postgres,
        );
        sqlx::query(&sql)
            .bind(&p.id)
            .bind(&p.content)
            .bind(&p.storage_key)
            .bind(&p.filename)
            .bind(&p.content_type)
            .bind(p.size)
            .bind(p.created_at)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get(&self, id: &str) -> Result<Option<Paste>> {
        let sql = q(
            "SELECT id, content, storage_key, filename, content_type, size, created_at
             FROM pastes WHERE id = ?",
            self.postgres,
        );
        let row = sqlx::query(&sql).bind(id).fetch_optional(&self.pool).await?;
        Ok(row.map(|r| Paste {
            id: r.get("id"),
            content: r.get("content"),
            storage_key: r.get("storage_key"),
            filename: r.get("filename"),
            content_type: r.get("content_type"),
            size: r.get("size"),
            created_at: r.get("created_at"),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::q;

    #[test]
    fn rewrites_placeholders_only_for_postgres() {
        let sql = "INSERT INTO pastes (id, size) VALUES (?, ?)";
        assert_eq!(q(sql, false), sql);
        assert_eq!(q(sql, true), "INSERT INTO pastes (id, size) VALUES ($1, $2)");
        assert_eq!(q("SELECT 1", true), "SELECT 1");
    }
}
