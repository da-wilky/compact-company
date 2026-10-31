package config

import (
	"fmt"
	"regexp"
	"strconv"
	"strings"
)

var seqPattern = regexp.MustCompile(`\{seq(?::0(\d+))?\}`)

func (c Config) CustomerNumber(seq int64) (string, error) {
	number, ok := substituteSeq(c.CustomerNumberFormat, seq)
	if !ok {
		return "", fmt.Errorf("customer_number_format: must contain {seq} or {seq:0N} exactly once, got %q", c.CustomerNumberFormat)
	}
	return number, nil
}

func (i Invoice) Number(year int, seq int64) (string, error) {
	return yearlyNumber("invoice.number_format", i.NumberFormat, year, seq)
}

func (q Quote) Number(year int, seq int64) (string, error) {
	return yearlyNumber("quote.number_format", q.NumberFormat, year, seq)
}

func yearlyNumber(key, format string, year int, seq int64) (string, error) {
	number, ok := substituteSeq(strings.Replace(format, "{year}", strconv.Itoa(year), 1), seq)
	if !ok || strings.Count(format, "{year}") != 1 {
		return "", fmt.Errorf("%s: must contain {year} and {seq} or {seq:0N} exactly once each, got %q", key, format)
	}
	return number, nil
}

// substituteSeq is false unless format contains {seq} or {seq:0N} exactly once.
func substituteSeq(format string, seq int64) (string, bool) {
	locs := seqPattern.FindAllStringSubmatchIndex(format, -1)
	if len(locs) != 1 {
		return "", false
	}
	m := locs[0]
	width := 0
	if m[2] >= 0 {
		width, _ = strconv.Atoi(format[m[2]:m[3]])
	}
	return format[:m[0]] + fmt.Sprintf("%0*d", width, seq) + format[m[1]:], true
}

func (c Config) validateNumberFormats() error {
	if _, err := c.CustomerNumber(1); err != nil {
		return err
	}
	// Invoice and quote numbers are file names under invoices/<year>/ and quotes/<year>/. Placeholders
	// only ever yield digits, so one sample number speaks for all of them.
	for _, f := range [][2]string{{"invoice.number_format", c.Invoice.NumberFormat}, {"quote.number_format", c.Quote.NumberFormat}} {
		number, err := yearlyNumber(f[0], f[1], 2026, 1)
		if err != nil {
			return err
		}
		if strings.Contains(number, "/") || strings.Contains(number, "..") {
			return fmt.Errorf(`%s: must not yield "/" or "..", got %q`, f[0], f[1])
		}
	}
	return nil
}
