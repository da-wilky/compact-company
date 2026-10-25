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
