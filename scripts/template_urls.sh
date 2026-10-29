#!/bin/sh
# Fails on a template URL that joins query parameters itself: built in Go (url.Values), each filter keeps the others the same way.
hits=$(grep -nE '(href|action|hx-get|hx-post)="([^"]*\?[^"]*(&[a-z_]+=|\{\{if)|[^"]*\{\{if[^"]*\?)' "${1:-internal/app/templates}"/*.html "${1:-internal/app/templates}"/partials/*.html)
[ -z "$hits" ] && exit 0
echo 'build URLs with more than one query parameter in Go (url.Values) and pass them to the template:' >&2
echo "$hits" >&2
exit 1
