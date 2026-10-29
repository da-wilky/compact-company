#!/bin/sh
# Fails on test assertions that match layout markup (CODING_STANDARDS.md).
# A line where the markup itself is what the user relies on ends in `// markup is the behaviour`.
hits=$(grep -nE 'want(Body|NoBody|InOrder|Row)\(.*(class=|</?(td|th|tr|thead|tbody|tfoot|table|span|div|section|nav|h[1-6]|colgroup|col|ul|li|p|br|svg)[ >])' "${1:-internal/app}"/*_test.go | grep -v '// markup is the behaviour$')
[ -z "$hits" ] && exit 0
echo "test assertions match layout markup; assert on text, links, form actions or form values (wantRow, text()):" >&2
echo "$hits" >&2
exit 1
