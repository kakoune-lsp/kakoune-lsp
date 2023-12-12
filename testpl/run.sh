#/usr/bin/env bash

export kak_quoted_selections=$(cat fuzz)

./script.pl > perl_output
./script.sh > sh_output

diff perl_output sh_output
