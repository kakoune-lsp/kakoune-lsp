#!/usr/bin/env bash

eval set -- "$kak_quoted_selections"

strip='s/^\${\?\|}//g'
split='^\([0-9]\+\):\(.\+\)$'

printf 'set-option window lsp_snippets_placeholder_groups'
while [ $# -gt 0 ]; do
    # Strip ${ and } from selection
    sel=$(printf '%s' "$1" | sed "$strip")
    # Split by :
    placeholder_id=$(printf '%s' "$sel" | sed -n "s/$split/\1/gp")
    if [ -z "$placeholder_id" ]; then
        # sed failed; There's no default placeholder.
        placeholder_id="$sel"
    fi
    if [ "$placeholder_id" -eq 0 ]; then
        placeholder_id=9999
    fi
    printf ' %s' "$placeholder_id"
    shift
done
printf '\n'

# No arrays in POSIX sh so we just loop again
eval set -- "$kak_quoted_selections"

printf 'set-register dquote'
while [ $# -gt 0 ]; do
    # Strip ${ and } from selection
    sel=$(printf '%s' "$1" | sed "$strip")
    # Split by :
    placeholder_id=$(printf '%s' "$sel" | sed -n "s/$split/\1/gp")
    def=$(printf '%s' "$sel" | sed -n "s/$split/\2/gp")
    if [ -n "$def" ]; then
        def=$(printf '%s' "$def" | sed "s/}}/}/g; s/'/''/g")
    else # if [ -z "$def" ]; then
        def=' '
    fi
    printf " '%s'" "$def"
    shift
done
printf '\n'
