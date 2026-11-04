#!/bin/sh
# Fails on a quote file using an invoice key or the reverse: rewording one would silently reword the other.
# Wording both share lives under common.*, lines.* or pdf.*; invoices.status.* names an invoice's own status.
d=${1:-internal/app}
hits=$( {
  grep -nE '"invoices\.' "$d"/quote*.go "$d"/snippet*.go "$d"/templates/quote*.html "$d"/templates/snippet*.html | grep -vE '"invoices\.status\.[a-z_]+"'
  grep -nE '"(quotes|snippets)\.' "$d"/invoice*.go "$d"/issue.go "$d"/templates/invoice*.html
} | grep -v '_test\.go:')
[ -z "$hits" ] && exit 0
echo 'quote and invoice files use their own catalog keys; move shared wording to common.*, lines.* or pdf.*:' >&2
echo "$hits" >&2
exit 1
