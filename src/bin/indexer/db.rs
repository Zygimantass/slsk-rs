//! SQLite database for the file index.

use rusqlite::{Connection, params};
use slsk_rs::peer::SharedDirectory;
use std::path::Path;

pub struct Database {
    conn: Connection,
}

pub struct SearchResult {
    pub username: String,
    pub filename: String,
    pub size: u64,
}

pub struct IndexStats {
    pub user_count: u64,
    pub file_count: u64,
    pub db_size_bytes: u64,
}

impl Database {
    pub fn open<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let conn = Connection::open(path.as_ref())?;

        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS users (
                id INTEGER PRIMARY KEY,
                username TEXT UNIQUE NOT NULL,
                indexed_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS files (
                id INTEGER PRIMARY KEY,
                user_id INTEGER NOT NULL,
                directory TEXT NOT NULL,
                filename TEXT NOT NULL,
                full_path TEXT NOT NULL,
                size INTEGER NOT NULL,
                extension TEXT,
                FOREIGN KEY (user_id) REFERENCES users(id)
            );

            CREATE INDEX IF NOT EXISTS idx_files_filename ON files(filename);
            CREATE INDEX IF NOT EXISTS idx_files_extension ON files(extension);
            CREATE INDEX IF NOT EXISTS idx_files_full_path ON files(full_path);
            CREATE INDEX IF NOT EXISTS idx_users_username ON users(username);
            ",
        )?;

        Ok(Self { conn })
    }

    pub fn index_user(&self, username: &str, directories: &[SharedDirectory]) -> anyhow::Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        // Delete existing data for this user
        self.conn.execute(
            "DELETE FROM files WHERE user_id = (SELECT id FROM users WHERE username = ?)",
            params![username],
        )?;

        // Insert or update user
        self.conn.execute(
            "INSERT INTO users (username, indexed_at) VALUES (?, ?)
             ON CONFLICT(username) DO UPDATE SET indexed_at = excluded.indexed_at",
            params![username, now],
        )?;

        let user_id: i64 = self.conn.query_row(
            "SELECT id FROM users WHERE username = ?",
            params![username],
            |row| row.get(0),
        )?;

        // Insert files in a transaction for speed
        self.conn.execute("BEGIN TRANSACTION", [])?;
        
        let mut stmt = self.conn.prepare_cached(
            "INSERT INTO files (user_id, directory, filename, full_path, size, extension)
             VALUES (?, ?, ?, ?, ?, ?)",
        )?;

        for dir in directories {
            for file in &dir.files {
                let filename = file
                    .filename
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or(&file.filename);

                let extension = filename
                    .rsplit('.')
                    .next()
                    .filter(|ext| ext.len() <= 10)
                    .map(|s| s.to_lowercase());

                stmt.execute(params![
                    user_id,
                    dir.path,
                    filename,
                    file.filename,
                    file.size as i64,
                    extension,
                ])?;
            }
        }
        
        drop(stmt);
        self.conn.execute("COMMIT", [])?;

        Ok(())
    }
    
    pub fn index_users_batch(&mut self, users: Vec<(String, Vec<SharedDirectory>)>) -> anyhow::Result<(u32, u32)> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let tx = self.conn.transaction()?;
        let mut success = 0u32;
        let mut failed = 0u32;

        for (username, directories) in users {
            // Delete existing data for this user
            tx.execute(
                "DELETE FROM files WHERE user_id = (SELECT id FROM users WHERE username = ?)",
                params![&username],
            )?;

            // Insert or update user
            tx.execute(
                "INSERT INTO users (username, indexed_at) VALUES (?, ?)
                 ON CONFLICT(username) DO UPDATE SET indexed_at = excluded.indexed_at",
                params![&username, now],
            )?;

            let user_id: i64 = tx.query_row(
                "SELECT id FROM users WHERE username = ?",
                params![&username],
                |row| row.get(0),
            )?;

            // Insert files
            let mut stmt = tx.prepare_cached(
                "INSERT INTO files (user_id, directory, filename, full_path, size, extension)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )?;

            for dir in &directories {
                for file in &dir.files {
                    let filename = file
                        .filename
                        .rsplit(['/', '\\'])
                        .next()
                        .unwrap_or(&file.filename);

                    let extension = filename
                        .rsplit('.')
                        .next()
                        .filter(|ext| ext.len() <= 10)
                        .map(|s| s.to_lowercase());

                    if stmt.execute(params![
                        user_id,
                        dir.path,
                        filename,
                        file.filename,
                        file.size as i64,
                        extension,
                    ]).is_err() {
                        failed += 1;
                        continue;
                    }
                }
            }
            
            success += 1;
        }

        tx.commit()?;
        Ok((success, failed))
    }

    pub fn search(&self, query: &str, limit: usize) -> anyhow::Result<Vec<SearchResult>> {
        // Split query into words and search for all of them
        let words: Vec<&str> = query.split_whitespace().collect();
        if words.is_empty() {
            return Ok(vec![]);
        }

        // Build WHERE clause for all words
        let conditions: Vec<String> = words
            .iter()
            .map(|_| "full_path LIKE ?".to_string())
            .collect();
        let where_clause = conditions.join(" AND ");

        let sql = format!(
            "SELECT u.username, f.full_path, f.size
             FROM files f
             JOIN users u ON f.user_id = u.id
             WHERE {}
             ORDER BY f.size DESC
             LIMIT ?",
            where_clause
        );

        let mut stmt = self.conn.prepare(&sql)?;

        // Bind parameters
        let patterns: Vec<String> = words.iter().map(|w| format!("%{}%", w)).collect();
        let mut params_vec: Vec<&dyn rusqlite::ToSql> = patterns
            .iter()
            .map(|s| s as &dyn rusqlite::ToSql)
            .collect();
        let limit_i64 = limit as i64;
        params_vec.push(&limit_i64);

        let results = stmt
            .query_map(rusqlite::params_from_iter(params_vec), |row| {
                Ok(SearchResult {
                    username: row.get(0)?,
                    filename: row.get(1)?,
                    size: row.get::<_, i64>(2)? as u64,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(results)
    }

    pub fn get_indexed_users(&self) -> anyhow::Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT username FROM users")?;
        let users = stmt
            .query_map([], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(users)
    }

    pub fn get_stats(&self) -> anyhow::Result<IndexStats> {
        let user_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))?;

        let file_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))?;

        let page_count: i64 = self
            .conn
            .query_row("PRAGMA page_count", [], |row| row.get(0))?;
        let page_size: i64 = self
            .conn
            .query_row("PRAGMA page_size", [], |row| row.get(0))?;

        Ok(IndexStats {
            user_count: user_count as u64,
            file_count: file_count as u64,
            db_size_bytes: (page_count * page_size) as u64,
        })
    }
}
