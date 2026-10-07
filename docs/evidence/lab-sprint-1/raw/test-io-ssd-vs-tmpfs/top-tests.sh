#!/bin/bash
# The 5 tests with the largest ssd-tmpfs difference in the suite runs, each run
# alone (test binary, --exact): 3 interleaved timed runs per configuration,
# then one `strace -f -c` run per configuration.
export LC_ALL=C
OUT=$1; BIN=$(cat $OUT/test-binary.txt); LOG=$OUT/top-tests-log.txt
TESTS="storage::fault::tests::crash_keeps_or_loses_an_unsynced_create_and_rename_in_order
storage::btree::tests::a_million_random_puts_and_deletes_match_a_btreemap_model
storage::btree::tests::a_tree_ten_times_larger_than_the_cap_reads_back_intact
suites::fault::migration
tx::commit::tests::an_undone_batch_that_took_an_emptied_trunk_leaves_the_free_list_intact"
cd /data/dev/xumiga/lpg
setup(){ rm -rf /tmp/lpgram; if [ $1 = ssd ]; then mkdir -p /data/tmp/lpg-lab/lpgram; ln -s /data/tmp/lpg-lab/lpgram /tmp/lpgram; else mkdir -p /tmp/lpgram; fi; }
restore(){ [ -L /tmp/lpgram ] && rm /tmp/lpgram; rm -rf /tmp/lpgram /data/tmp/lpg-lab/lpgram; mkdir -p /tmp/lpgram; }
i=0
for t in $TESTS; do
 i=$((i+1))
 for rep in 1 2 3; do for cfg in tmpfs ssd; do
  setup $cfg
  s=$(date +%s.%N); $BIN "$t" --exact --test-threads=1 >/dev/null 2>&1; rc=$?; e=$(date +%s.%N)
  echo "t$i $cfg rep$rep rc=$rc wall_s=$(echo "$e - $s" | bc) loadavg=$(cut -d' ' -f1 /proc/loadavg) $t" >>$LOG
  restore
 done; done
 for cfg in tmpfs ssd; do
  setup $cfg
  strace -f -c -o $OUT/strace-c-t$i-$cfg.txt $BIN "$t" --exact --test-threads=1 >/dev/null 2>&1
  echo "t$i $cfg strace rc=$? $t" >>$LOG
  restore
 done
done
