# Lists a whole tree for KitchenSync in one pass (specs/sync.md, "Listing A
# Whole Tree"). Run as `perl - <root>` with this script on standard input.
# For each directory, in the order the sync walk visits them (a directory's
# entries, then its subdirectories, names sorted case-insensitively with the
# exact name breaking ties), prints one record per regular file or
# subdirectory and then an end record, each ended by a NUL byte:
#   f<TAB>size<TAB>mtime<TAB>path     a file
#   d<TAB>-1<TAB>mtime<TAB>path       a directory
#   e<TAB>-<TAB>-<TAB>dir             the end of dir's listing ("" is the root)
# mtime is in seconds; paths are relative to the root. .kitchensync and .git
# are listed but not entered. A directory that cannot be read gets no end
# record, and the program then exits 2.
use strict;
use warnings;
use IO::Handle;
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
    my @names = sort { lc($a) cmp lc($b) or $a cmp $b } grep { $_ ne '.' && $_ ne '..' } readdir($dh);
    closedir($dh);
    my @subdirs;
    for my $name (@names) {
        my @s = Time::HiRes::lstat("$dir/$name") or next;
        my $path = $rel eq '' ? $name : "$rel/$name";
        if (-d _) {
            print "d\t-1\t$s[9]\t$path\0";
            push @subdirs, $path unless $name eq '.kitchensync' || $name eq '.git';
        } elsif (-f _) {
            print "f\t$s[7]\t$s[9]\t$path\0";
        }
    }
    print "e\t-\t-\t$rel\0";
    STDOUT->flush;
    walk($_) for @subdirs;
}
walk('');
exit($failed ? 2 : 0);
