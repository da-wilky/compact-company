package app

import (
	"testing"

	"golang.org/x/text/currency"
)

func TestParseAmount(t *testing.T) {
	for _, tc := range []struct {
		in, cur string
		want    int64
	}{
		{"85", "EUR", 8500},
		{"85.5", "EUR", 8550},
		{"85,50", "EUR", 8550},
		{" 0.05 ", "EUR", 5},
		{"8500", "JPY", 8500},
		{"1.234", "BHD", 1234},
	} {
		got, err := parseAmount(tc.in, currency.MustParseISO(tc.cur))
		if err != nil || got != tc.want {
			t.Errorf("parseAmount(%q, %s) = %d, %v; want %d", tc.in, tc.cur, got, err, tc.want)
		}
	}
}

func TestParseAmountRejects(t *testing.T) {
	for _, tc := range []struct{ in, cur string }{
		{"", "EUR"},
		{"-5", "EUR"},
		{"85.555", "EUR"},
		{"1.5", "JPY"},
		{"1.", "EUR"},
		{".5", "EUR"},
		{"1,000.00", "EUR"},
		{"abc", "EUR"},
		{"99999999999999999999", "EUR"},
	} {
		if got, err := parseAmount(tc.in, currency.MustParseISO(tc.cur)); err == nil {
			t.Errorf("parseAmount(%q, %s) = %d, want error", tc.in, tc.cur, got)
		}
	}
}

func TestFormatAmount(t *testing.T) {
	for _, tc := range []struct {
		minor int64
		cur   string
		want  string
	}{
		{8550, "EUR", "85.50"},
		{5, "EUR", "0.05"},
		{8500, "JPY", "8500"},
		{1234, "BHD", "1.234"},
	} {
		if got := formatAmount(tc.minor, currency.MustParseISO(tc.cur)); got != tc.want {
			t.Errorf("formatAmount(%d, %s) = %q, want %q", tc.minor, tc.cur, got, tc.want)
		}
	}
}

func TestRescaleAmount(t *testing.T) {
	for _, tc := range []struct {
		minor    int64
		from, to string
		want     int64
	}{
		{10000, "EUR", "JPY", 100},
		{10050, "EUR", "JPY", 101},
		{10049, "EUR", "JPY", 100},
		{1449, "BHD", "JPY", 1},
		{100, "JPY", "EUR", 10000},
		{100, "JPY", "BHD", 100000},
		{8550, "EUR", "USD", 8550},
	} {
		if got := rescaleAmount(tc.minor, currency.MustParseISO(tc.from), currency.MustParseISO(tc.to)); got != tc.want {
			t.Errorf("rescaleAmount(%d, %s, %s) = %d, want %d", tc.minor, tc.from, tc.to, got, tc.want)
		}
	}
}
