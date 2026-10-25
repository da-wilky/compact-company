package config

import (
	"fmt"
	"time"

	"github.com/BurntSushi/toml"
)

type Config struct {
	Listen               string   `toml:"listen"`
	DataDir              string   `toml:"data_dir"`
	CustomerNumberFormat string   `toml:"customer_number_format"`
	Auth                 Auth     `toml:"auth"`
	Defaults             Defaults `toml:"defaults"`
}

type Auth struct {
	Mode               string        `toml:"mode"`
	SessionIdleTimeout time.Duration `toml:"session_idle_timeout"`
}

type Defaults struct {
	Currency        string `toml:"currency"`
	Language        string `toml:"language"`
	Timezone        string `toml:"timezone"`
	ParallelEntries bool   `toml:"parallel_entries"`
}

func Load(path string) (Config, error) {
	cfg := Config{
		Listen:               "127.0.0.1:8080",
		CustomerNumberFormat: "K-{seq:03}",
		Auth:                 Auth{SessionIdleTimeout: 14 * 24 * time.Hour},
		Defaults:             Defaults{Currency: "EUR", Language: "en", Timezone: "UTC"},
	}
	if _, err := toml.DecodeFile(path, &cfg); err != nil {
		return Config{}, fmt.Errorf("config %s: %w", path, err)
	}
	return cfg, nil
}
