#!/bin/bash
# show 旗標邊界
. /Users/howie/.claude/jobs/9eb90dff/tmp/w12/lib.sh
cd $W12/jails/p16
oracle show --item-type bogus
oracle show ch --item-type=
oracle show ch -rr
oracle show ch --deltas-only --deltas-only
oracle show ch --item-type spec --item-type change
oracle show ch --json --item-type spec
oracle show ch --item-type change --diff
