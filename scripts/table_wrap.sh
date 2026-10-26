#!/bin/sh
# Fails on a template <table> outside .table-wrap: on a phone it widens the whole page instead of scrolling.
hits=$(awk '/<table/ && !(substr($0, 1, index($0, "<table")) ~ /table-wrap/ || prev ~ /table-wrap/) { print FILENAME ":" FNR ": " $0 } { prev = $0 }' "${1:-internal/app/templates}"/*.html "${1:-internal/app/templates}"/partials/*.html)
[ -z "$hits" ] && exit 0
echo 'wrap each <table> in <div class="table-wrap">:' >&2
echo "$hits" >&2
exit 1
