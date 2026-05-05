#!/usr/bin/perl
use POSIX qw(strftime);
$|=1;
my $log_dir = shift || ".";
my $curr = "";
my $fh;
while(<STDIN>) {
    my $d = strftime("%Y%m%d", localtime);
    if ($d ne $curr) {
        $curr = $d;
        my $dir = "$log_dir/$d";
        system("mkdir -p $dir") unless -d $dir;
        close($fh) if $fh;
        open($fh, ">>", "$dir/server_$d.log");
        my $ofh = select($fh); $| = 1; select($ofh);
    }
    print $fh $_;
}
