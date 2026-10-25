package db

import (
	"database/sql"
	"embed"
	"fmt"
	"io/fs"
	"net/url"
	"sort"
	"strconv"
	"strings"

	_ "modernc.org/sqlite"
)

//go:embed migrations/*.sql
var migrations embed.FS

// Open opens the SQLite database at path in WAL mode and applies pending migrations.
func Open(path string) (*sql.DB, error) {
	dsn := "file:" + (&url.URL{Path: path}).EscapedPath() +
		"?_pragma=journal_mode(WAL)&_pragma=foreign_keys(1)&_pragma=busy_timeout(5000)&_txlock=immediate"
	db, err := sql.Open("sqlite", dsn)
	if err != nil {
		return nil, err
	}
	if err := migrate(db); err != nil {
		_ = db.Close()
		return nil, err
	}
	return db, nil
}

type migration struct {
	version int
	name    string
}

func migrate(db *sql.DB) error {
	pending, err := list()
	if err != nil {
		return err
	}
	var current int
	if err := db.QueryRow("PRAGMA user_version").Scan(&current); err != nil {
		return err
	}
	if current > len(pending) {
		return fmt.Errorf("database schema version %d is newer than this binary (%d)", current, len(pending))
	}
	for _, m := range pending[current:] {
		script, err := migrations.ReadFile("migrations/" + m.name)
		if err != nil {
			return err
		}
		if err := apply(db, m.version, string(script)); err != nil {
			return fmt.Errorf("migration %s: %w", m.name, err)
		}
	}
	return nil
}

func apply(db *sql.DB, version int, script string) error {
	tx, err := db.Begin()
	if err != nil {
		return err
	}
	defer func() { _ = tx.Rollback() }()
	if _, err := tx.Exec(script); err != nil {
		return err
	}
	if _, err := tx.Exec("PRAGMA user_version = " + strconv.Itoa(version)); err != nil {
		return err
	}
	return tx.Commit()
}

// list returns the embedded migrations, which must be numbered 1..n without gaps or duplicates.
func list() ([]migration, error) {
	entries, err := fs.ReadDir(migrations, "migrations")
	if err != nil {
		return nil, err
	}
	var ms []migration
	for _, e := range entries {
		prefix, _, _ := strings.Cut(e.Name(), "_")
		v, err := strconv.Atoi(prefix)
		if err != nil {
			return nil, fmt.Errorf("migration %s: name must start with a number", e.Name())
		}
		ms = append(ms, migration{version: v, name: e.Name()})
	}
	sort.Slice(ms, func(i, j int) bool { return ms[i].version < ms[j].version })
	for i, m := range ms {
		if m.version != i+1 {
			return nil, fmt.Errorf("migration %s: expected number %d (numbers must be unique and contiguous)", m.name, i+1)
		}
	}
	return ms, nil
}
