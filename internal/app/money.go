package app

import (
	"errors"
	"fmt"
	"regexp"
	"strconv"
	"strings"

	"golang.org/x/text/currency"
)

var amountPattern = regexp.MustCompile(`^(\d+)(?:[.,](\d+))?$`)

// parseAmount converts a non-negative decimal in cur into cur's minor unit.
func parseAmount(s string, cur currency.Unit) (int64, error) {
	scale, _ := currency.Standard.Rounding(cur)
	m := amountPattern.FindStringSubmatch(strings.TrimSpace(s))
	if m == nil {
		return 0, errors.New("not a decimal number")
	}
	if len(m[2]) > scale {
		return 0, fmt.Errorf("at most %d decimals", scale)
	}
	minor, err := strconv.ParseInt(m[1]+m[2]+strings.Repeat("0", scale-len(m[2])), 10, 64)
	if err != nil {
		return 0, err
	}
	return minor, nil
}

// rescaleAmount keeps the decimal value of a non-negative minor amount, rounding half up.
func rescaleAmount(minor int64, from, to currency.Unit) int64 {
	fromScale, _ := currency.Standard.Rounding(from)
	toScale, _ := currency.Standard.Rounding(to)
	p := int64(1)
	for range max(toScale-fromScale, fromScale-toScale) {
		p *= 10
	}
	if toScale > fromScale {
		return minor * p
	}
	return (minor + p/2) / p
}

func formatAmount(minor int64, cur currency.Unit) string {
	scale, _ := currency.Standard.Rounding(cur)
	s := fmt.Sprintf("%0*d", scale+1, minor)
	if scale == 0 {
		return s
	}
	return s[:len(s)-scale] + "." + s[len(s)-scale:]
}
