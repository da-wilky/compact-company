package config

import (
	"errors"
	"fmt"
	"net"
	"net/url"
	"os"
	"slices"
	"strings"
	"time"

	"github.com/BurntSushi/toml"
	"golang.org/x/text/currency"
)

type Config struct {
	Listen               string   `toml:"listen"`
	PublicURL            string   `toml:"public_url"`
	DataDir              string   `toml:"data_dir"`
	CustomerNumberFormat string   `toml:"customer_number_format"`
	Auth                 Auth     `toml:"auth"`
	Defaults             Defaults `toml:"defaults"`
	Invoice              Invoice  `toml:"invoice"`
	Business             Business `toml:"business"`
	BusinessFile         string   `toml:"business_file"`
	BusinessManaged      bool     `toml:"-"`
}

type Business struct {
	Name      string   `toml:"name"`
	Company   string   `toml:"company"`
	Address   []string `toml:"address"`
	Email     string   `toml:"email"`
	Web       string   `toml:"web"`
	Phone     string   `toml:"phone"`
	TaxNumber string   `toml:"tax_number"`
	Bank      Bank     `toml:"bank"`
	TaxMode   string   `toml:"tax_mode"`
	VATRate   int      `toml:"vat_rate"`
}

type Bank struct {
	Name string `toml:"name"`
	IBAN string `toml:"iban"`
	BIC  string `toml:"bic"`
}

type Auth struct {
	Mode               string        `toml:"mode"`
	SessionIdleTimeout time.Duration `toml:"session_idle_timeout"`
	Issuer             string        `toml:"issuer"`
	ClientID           string        `toml:"client_id"`
	ClientSecretFile   string        `toml:"client_secret_file"`
	AllowedSubjects    []string      `toml:"allowed_subjects"`
	ClientSecret       string        `toml:"-"`
}

type Invoice struct {
	PaymentDays     int `toml:"payment_days"`
	RoundingMinutes int `toml:"rounding_minutes"`
}

type Defaults struct {
	Currency        string `toml:"currency"`
	Language        string `toml:"language"`
	Timezone        string `toml:"timezone"`
	ParallelEntries bool   `toml:"parallel_entries"`
}

var RoundingIncrements = []int{0, 5, 10, 15, 30}

func Load(path string) (Config, error) {
	cfg := Config{
		Listen:               "127.0.0.1:8080",
		CustomerNumberFormat: "K-{seq:03}",
		Auth:                 Auth{SessionIdleTimeout: 14 * 24 * time.Hour},
		Defaults:             Defaults{Currency: "EUR", Language: "en", Timezone: "UTC"},
		Invoice:              Invoice{PaymentDays: 14},
	}
	md, err := decodeStrict(path, &cfg)
	if err != nil {
		return Config{}, fmt.Errorf("config %s: %w", path, err)
	}
	cfg.BusinessManaged = md.IsDefined("business") || cfg.BusinessFile != ""
	if err := cfg.validate(); err != nil {
		return Config{}, fmt.Errorf("config %s: %w", path, err)
	}
	return cfg, nil
}

func decodeStrict(path string, v any) (toml.MetaData, error) {
	md, err := toml.DecodeFile(path, v)
	if err != nil {
		return md, err
	}
	if undecoded := md.Undecoded(); len(undecoded) > 0 {
		keys := make([]string, len(undecoded))
		for i, k := range undecoded {
			keys[i] = k.String()
		}
		return md, fmt.Errorf("unknown keys: %s", strings.Join(keys, ", "))
	}
	return md, nil
}

func (c *Config) validate() error {
	if c.DataDir == "" {
		return errors.New("data_dir is required")
	}
	if c.Auth.SessionIdleTimeout <= 0 {
		return errors.New("auth.session_idle_timeout must be positive")
	}
	switch c.Auth.Mode {
	case "none":
		if !isLoopback(c.Listen) {
			return fmt.Errorf("auth.mode = \"none\" requires a loopback listen address, got %q", c.Listen)
		}
	case "oidc":
		if u, err := url.Parse(c.PublicURL); err != nil || u.Scheme == "" || u.Host == "" {
			return fmt.Errorf("public_url must be an absolute URL in oidc mode, got %q", c.PublicURL)
		}
		c.PublicURL = strings.TrimSuffix(c.PublicURL, "/")
		if c.Auth.Issuer == "" {
			return errors.New("auth.issuer is required in oidc mode")
		}
		if c.Auth.ClientID == "" {
			return errors.New("auth.client_id is required in oidc mode")
		}
		if c.Auth.ClientSecretFile == "" {
			return errors.New("auth.client_secret_file is required in oidc mode")
		}
		secret, err := os.ReadFile(c.Auth.ClientSecretFile)
		if err != nil {
			return fmt.Errorf("auth.client_secret_file: %w", err)
		}
		c.Auth.ClientSecret = strings.TrimSpace(string(secret))
	default:
		return fmt.Errorf(`auth.mode must be "oidc" or "none", got %q`, c.Auth.Mode)
	}
	if _, err := time.LoadLocation(c.Defaults.Timezone); err != nil || c.Defaults.Timezone == "" || c.Defaults.Timezone == "Local" {
		return fmt.Errorf("defaults.timezone: unknown timezone %q", c.Defaults.Timezone)
	}
	if _, err := currency.ParseISO(c.Defaults.Currency); err != nil {
		return fmt.Errorf("defaults.currency: unknown ISO 4217 code %q", c.Defaults.Currency)
	}
	if !slices.Contains(RoundingIncrements, c.Invoice.RoundingMinutes) {
		return fmt.Errorf("invoice.rounding_minutes must be one of %v, got %d", RoundingIncrements, c.Invoice.RoundingMinutes)
	}
	if c.Invoice.PaymentDays <= 0 {
		return fmt.Errorf("invoice.payment_days must be positive, got %d", c.Invoice.PaymentDays)
	}
	if c.BusinessManaged {
		return c.loadBusiness()
	}
	return nil
}

// loadBusiness decodes business_file over [business], so the file's keys win key by key.
func (c *Config) loadBusiness() error {
	if c.BusinessFile != "" {
		if _, err := decodeStrict(c.BusinessFile, &c.Business); err != nil {
			return fmt.Errorf("business_file %s: %w", c.BusinessFile, err)
		}
	}
	if c.Business.TaxMode == "" {
		c.Business.TaxMode = "small_business"
	}
	if c.Business.TaxMode != "small_business" && c.Business.TaxMode != "standard" {
		return fmt.Errorf(`business.tax_mode must be "small_business" or "standard", got %q`, c.Business.TaxMode)
	}
	if c.Business.VATRate < 0 || c.Business.VATRate > 100 {
		return fmt.Errorf("business.vat_rate must be 0 to 100, got %d", c.Business.VATRate)
	}
	return nil
}

func isLoopback(listen string) bool {
	host, _, err := net.SplitHostPort(listen)
	if err != nil {
		return false
	}
	if host == "localhost" {
		return true
	}
	ip := net.ParseIP(host)
	return ip != nil && ip.IsLoopback()
}
