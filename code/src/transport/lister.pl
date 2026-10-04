# Lists a whole tree for KitchenSync in one pass (specs/sync.md, "Listing A
# Whole Tree"). Run as `perl - <root>` with this script on standard input.
# Prints one record per regular file or directory, each ended by a NUL byte:
# kind (f or d), a tab, size in bytes (-1 for a directory), a tab, the
# modification time in seconds, a tab, and the path relative to the root.
# .kitchensync and .git are listed but not entered. Exits 2 if any directory
# could not be read, so a partial listing is never mistaken for a full one.
use strict;
use warnings;
use Time::HiRes ();
binmode STDOUT;
my $root = shift;
my $failed = 0;
sub walk {
    my ($rel) = @_;
    my $dir = $rel eq '' ? $root : "$root/$rel";
    my $dh;
    unless (opendir($dh, $dir)) {
        print STDERR "$dir: $!\n";
        $failed = 1;
        return;
    }
    my @names = grep { $_ ne '.' && $_ ne '..' } readdir($dh);
    closedir($dh);
    for my $name (@names) {
        my @s = Time::HiRes::lstat("$dir/$name") or next;
        my $path = $rel eq '' ? $name : "$rel/$name";
        if (-d _) {
            print "d\t-1\t$s[9]\t$path\0";
            walk($path) unless $name eq '.kitchensync' || $name eq '.git';
        } elsif (-f _) {
            print "f\t$s[7]\t$s[9]\t$path\0";
        }
    }
}
walk('');
exit($failed ? 2 : 0);
