# IN-PROGRESS: DR3 mag-20 excerpt build

> **This file is temporary.** Delete it from `main` once the DR3 mag-20 excerpt
> has finished and we've moved on. It exists so a future Claude Code session
> (or you, returning to this after a break) can pick up where we left off
> without re-deriving context.

## What's running

A `gaia-excerpt` invocation is streaming all 3,386 DR3 `gaia_source` files
(~750 GB compressed) through the new `--cache-raw --clean-after-excerpt`
atomic-per-file pipeline, magnitude-cut to G < 20, sharded into 128 HEALPix-5
files.

```
command:    gaia-excerpt \
              --from-release dr3 --cache-raw --clean-after-excerpt \
              --output-dir ~/.cache/starfield/gaia-excerpts/dr3-mag20/ \
              --shard-by healpix --healpix-level 5 --shards 128 \
              --mag-limit 20

binary:     ~/repos/starfield-datasources/target/release/gaia-excerpt
            (built from this branch)

output:     ~/.cache/starfield/gaia-excerpts/dr3-mag20/shard_NNNN.csv.gz × 128
log:        ~/.cache/starfield/gaia-excerpts/dr3-mag20.log
raw cache:  ~/.cache/starfield/gaia/dr3/   (transient; ~250 MB / 1–2 files in flight)

ETA:        ~25–30 hours from launch (network-bound on ESA's CDN)
```

To find the live PID: `pgrep -af gaia-excerpt | grep -v pgrep`.

## Why this approach (and what failed first)

Two earlier full-catalog attempts failed for distinct reasons:

1. **Pure streaming** (`--from-release dr3` with no `--cache-raw`): mid-stream
   HTTP body errors during the streamed gz decode caused unrecoverable row
   loss. ~18 files × ~150k rows lost in run 2. Streaming has no MD5
   verification and no resume; the only safe per-file retry is
   "give up if any rows already committed", which is exactly the data-loss
   case we wanted to avoid.

2. **Per-file `ShardedCsvWriter`**: the convenience function `excerpt_csv_file`
   constructed a fresh writer on every input. The writer's first-touch on a
   shard called `File::create()`, which **truncates**. After ~10 hours the
   total disk size was *shrinking* because each new input was overwriting
   prior shards. Final state was useless — at most one input file's worth of
   data per shard.

The current approach uses **download-then-extract-then-evict**, with **one
writer that lives across the whole run**:

- `Downloader::download_file` writes to `.tmp` + atomic rename + MD5 verify;
  partial / interrupted downloads re-fetch cleanly on the next attempt.
- `excerpt_csv_file_into` parses the local file into the shared writer.
  Local I/O can't fail mid-stream the way HTTP can.
- `--clean-after-excerpt` deletes the cached raw file immediately after a
  successful extract, so disk for raw stays bounded.
- The shared `ShardedCsvWriter` is created once per run; per-file shard files
  are opened lazily on first touch and never re-truncated.

## How to check on it

```sh
# alive?
pgrep -af gaia-excerpt | grep -v pgrep
ps -p $PID -o pid,etime,rss,stat,%cpu

# output
ls ~/.cache/starfield/gaia-excerpts/dr3-mag20/ | wc -l   # should be 128
du -sh ~/.cache/starfield/gaia-excerpts/dr3-mag20/

# raw cache (should hover at 1–2 files)
ls ~/.cache/starfield/gaia/dr3/*.csv.gz 2>/dev/null | wc -l
du -sh ~/.cache/starfield/gaia/dr3/

# trouble in the log
grep -cE 'attempt [0-9]+/5 failed' ~/.cache/starfield/gaia-excerpts/dr3-mag20.log
grep -E 'giving up' ~/.cache/starfield/gaia-excerpts/dr3-mag20.log
tail -20 ~/.cache/starfield/gaia-excerpts/dr3-mag20.log
```

## Expected end state on success

- `~/.cache/starfield/gaia-excerpts/dr3-mag20/` contains 128 shard files,
  total ~120–180 GB (mag-20 cut of DR3's ~1.5B sources; per-shard size
  varies 600 MB – 1.7 GB based on HEALPix density across the sky).
- `~/.cache/starfield/gaia/dr3/` is empty.
- Log's final stdout line: `read N stars; kept M (Z%); wrote 128 shard files`.
- No `giving up` lines in the log. Some `attempt 1/5 failed ... retrying` is
  normal and harmless.

## If it died

1. `tail -50 ~/.cache/starfield/gaia-excerpts/dr3-mag20.log` for the last error.
2. The cached raw file for whatever was in flight is still in
   `~/.cache/starfield/gaia/dr3/`. Re-running the same command will pick up
   from there: `download_file` skips files already cached, eviction only
   happens on extract success.
3. If specific files in the log keep saying "giving up", their names are
   listed in the trailing summary. Re-run the same command and they'll be
   retried (they may succeed when ESA's CDN is healthier).
4. Worst case: `rm -rf ~/.cache/starfield/gaia-excerpts/dr3-mag20/` and re-run
   from scratch. ~25–30 hours.

## Next tasks queued behind this

1. Delete this file as part of the PR that closes out the DR3 build.
2. Hipparcos supplement-builder binary (`gaia-tools`): generate per-release
   supplement CSVs that get baked into the `starfield-gaia` crate so
   `Dr{N}Catalog::insert_missing_sources()` can fix Gaia's bright-end
   incompleteness without runtime cross-matching. Design discussed in chat;
   not yet started.
