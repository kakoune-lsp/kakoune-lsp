#!/usr/bin/env perl
use strict;
use warnings;
use Text::ParseWords();

my @sel_content = Text::ParseWords::shellwords($ENV{"kak_quoted_selections"});

my %placeholder_id_to_default;
my @placeholder_ids;

print("set-option window lsp_snippets_placeholder_groups");
for my $i (0 .. $#sel_content) {
    my $sel = $sel_content[$i];
    $sel =~ s/\A\$\{?|\}\Z//g;
    my ($placeholder_id, $placeholder_default) = ($sel =~ /^(\d+)(?::(.*))?$/);
    if ($placeholder_id eq "0") {
        $placeholder_id = "9999";
    }
    $placeholder_ids[$i] = $placeholder_id;
    print(" $placeholder_id");
    if (defined($placeholder_default)) {
        $placeholder_id_to_default{$placeholder_id} = $placeholder_default;
    }
}
print("\n");

print("set-register dquote");
foreach my $i (0 .. $#sel_content) {
    my $placeholder_id = $placeholder_ids[$i];
    my $def = "";
    if (exists $placeholder_id_to_default{$placeholder_id}) {
        $def = $placeholder_id_to_default{$placeholder_id};
        # de-double up closing braces
        $def =~ s/\}\}/}/g;
        # double up single-quotes
        $def =~ s/'/''/g;
    }
    # make sure that the placeholder is non-empty so we can select it
    if (length $def == 0) { $def = " " }
    print(" '$def'");
}
print("\n");

