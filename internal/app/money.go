package app

import (
	"errors"
	"fmt"
	"regexp"
	"strconv"
	"strings"

	"golang.org/x/text/currency"
)

var decimalPattern = regexp.MustCompile(`^(\d+)(?:[.,](\d+))?$`)

// parseDecimal converts a non-negative decimal into an integer count of 10^-scale.
func parseDecimal(s string, scale int) (int64, error) {
	m := decimalPattern.FindStringSubmatch(strings.TrimSpace(s))
	if m == nil {
		return 0, errors.New("not a decimal number")
	}
	if len(m[2]) > scale {
		return 0, fmt.Errorf("at most %d decimals", scale)
	}
	return strconv.ParseInt(m[1]+m[2]+strings.Repeat("0", scale-len(m[2])), 10, 64)
}

func formatDecimal(n int64, scale int) string {
	sign := ""
	if n < 0 {
		sign, n = "-", -n
	}
	s := fmt.Sprintf("%0*d", scale+1, n)
	if scale == 0 {
		return sign + s
	}
	return sign + s[:len(s)-scale] + "." + s[len(s)-scale:]
}

// divRound divides by a positive d, rounding halves away from zero.
func divRound(n, d int64) int64 {
	if n < 0 {
		return -((-n + d/2) / d)
	}
	return (n + d/2) / d
}

// parseAmount converts a non-negative decimal in cur into cur's minor unit.
func parseAmount(s string, cur currency.Unit) (int64, error) {
	scale, _ := currency.Standard.Rounding(cur)
	return parseDecimal(s, scale)
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
	return divRound(minor, p)
}

func formatAmount(minor int64, cur currency.Unit) string {
	scale, _ := currency.Standard.Rounding(cur)
	return formatDecimal(minor, scale)
}

// Quantities are stored as thousandths.
const quantityScale = 3

func parseQuantity(s string) (int64, error) {
	return parseDecimal(s, quantityScale)
}

// formatQuantity drops trailing zeros: 1500 is "1.5".
func formatQuantity(q int64) string {
	return strings.TrimSuffix(strings.TrimRight(formatDecimal(q, quantityScale), "0"), ".")
}

// lineTotal is quantity × unit price in minor units, half up.
func lineTotal(quantity, unitPrice int64) int64 {
	return divRound(quantity*unitPrice, 1000)
}

type invoiceTotals struct {
	Net, VAT, Gross int64
}

func (t invoiceTotals) format(unit currency.Unit) totalsData {
	return totalsData{Net: formatAmount(t.Net, unit), VAT: formatAmount(t.VAT, unit), Gross: formatAmount(t.Gross, unit)}
}

// totalsOf rounds VAT once on the net total rather than per line.
func totalsOf(lineTotals []int64, vatPercent int64) invoiceTotals {
	var t invoiceTotals
	for _, lt := range lineTotals {
		t.Net += lt
	}
	t.VAT = divRound(t.Net*vatPercent, 100)
	t.Gross = t.Net + t.VAT
	return t
}
