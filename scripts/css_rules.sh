#!/bin/sh
# Fails on app.css rules that break CODING_STANDARDS.md: ID selectors, and colour literals outside the :root token blocks.
css=${1:-internal/app/static/app.css}
hits=$(awk '
  /:root[^{]*\{/ { inroot = 1 }
  inroot { if ($0 ~ /^[[:space:]]*}/) inroot = 0; next }
  index($0, "{") && substr($0, 1, index($0, "{")) ~ /#[A-Za-z_-]/ { print FILENAME ":" FNR ": ID selector: " $0; next }
  /#[0-9A-Fa-f]{3,8}([^0-9A-Za-z]|$)|(rgba?|hsla?)\(/ { print FILENAME ":" FNR ": colour literal: " $0 }
' "$css")
[ -z "$hits" ] && exit 0
echo "styles hang off a class, and colours come from the :root tokens:" >&2
echo "$hits" >&2
exit 1
